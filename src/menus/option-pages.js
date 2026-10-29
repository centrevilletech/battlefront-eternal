import { FLIGHT_BINDINGS } from '../flight/controls.js';
import { bindable, flatBindings, playerBindings, rebind } from "../input.js";
import { MenuScreen } from "./screen.js";
import { Button, List, OptionRow, Options, Panel, Rows, Tabs, Text } from "./widgets.js";
import { keyPopup } from "./popup.js";
import { PAGES, optionRow, optionsFor, resetPage } from "./options.js";
import { TABS, hotkeyRows } from "./hotkeys.js";

// The options menu and its pages, the same in the front end and over a
// paused battle:
// - OPTIONS: menu rows GAME / VIDEO / AUDIO / CONTROL OPTIONS (the front
//   end adds ONLINE OPTIONS and CREDITS), turned 40 degrees, nothing
//   dimmed behind;
// - each page dims what is behind it, lists its settings (options.js) as
//   label | value rows, and has BACK; VIDEO and AUDIO a RESET TO DEFAULTS
//   in the middle, GAME as its last row;
// - CONTROL OPTIONS: mouse sensitivity and invert Y in a window, then
//   CONFIG BINDINGS: the INFANTRY / VEHICLE / STARFIGHTER tabs over the
//   bindings list (hotkeys.js).
//
// settings: the Settings; strings: common.json's; inBattle: over a battle
// (the notes on changes that wait for a restart say RESTART MISSION).
// viewpoint: { get(), set(firstPerson) } for the GAME page's VIEWPOINT
// row (a battle's); audioPanel(): opens the speaker test and audio
// details, or null for no such row; online and credits: the front end's
// actions for those rows (absent: the rows are hidden, as in a battle);
// integrations: the desktop client's STEAM & DISCORD (src/shell/integrations.js).

const say = (strings, key, fallback) => String((key && strings[key]) || fallback).replace(/\r?\n/g, " ").trim();

// The original's ifs_opt_top layout: 8 rows in its list, 2 of them never
// shown on PC (console controls, special features).
const TOP_LAYOUT = { at: [0.6, 0.55], y: 20, rotY: 40, font: "gamefont_large", width: 360, ySpacing: 5 };
const TOP_COUNT = 8;

export function optionsMenu({ settings, strings = {}, inBattle = false, viewpoint = null, audioPanel = null, online = null, credits = null, integrations = null } = {}) {
  const context = { settings, strings, inBattle, viewpoint, audioPanel };
  const open = (build) => (screen) => screen.stack.push(build(context));
  const rows = new Rows({
    id: "options",
    title: say(strings, "ifs.Main.options", "OPTIONS"),
    layout: TOP_LAYOUT,
    listCount: TOP_COUNT,
    rows: [
      { id: "general", label: say(strings, "ifs.GameOpt.title", "GAME OPTIONS"), action: open(gamePage) },
      { id: "video", label: say(strings, "ifs.VideoOpt.title", "VIDEO OPTIONS"), action: open(videoPage) },
      { id: "sound", label: say(strings, "ifs.SoundOpt.title", "AUDIO OPTIONS"), action: open(audioPage) },
      { id: "controls", label: say(strings, "ifs.GameOpt.pc_optionstitle", "CONTROL OPTIONS"), action: open(controlsPage) },
      { id: "online", label: say(strings, "ifs.onlineopt.title", "ONLINE OPTIONS"), hidden: !online, action: (screen) => online?.(screen) },
      { id: "integrations", label: "STEAM & DISCORD", hidden: !integrations, action: (screen) => integrations?.(screen) },
      { id: "credits", label: say(strings, "ifs.credits.title", "CREDITS"), hidden: !credits, action: (screen) => credits?.(screen) },
    ],
  });
  return new MenuScreen({ id: "options", widgets: [rows, backButton(strings)], focus: "first" });
}

function backButton(strings) {
  return new Button({ id: "back", place: "back", text: say(strings, "common.back", "BACK") });
}

function title(page, strings, y) {
  const entry = PAGES.find((p) => p.id === page);
  return new Text({ id: "title", look: "title", text: say(strings, entry.titleKey, entry.title), y });
}

function resetButton(strings, page, settings) {
  return new Button({ id: "reset", place: "middle", text: say(strings, "common.reset", "RESET TO DEFAULTS"), action: () => resetPage(page, settings) });
}

// GAME OPTIONS: VIEWPOINT (in a battle), the settings, RESET TO DEFAULTS.
export function gamePage({ settings, strings, inBattle, viewpoint }) {
  const rows = [];
  if (viewpoint) {
    rows.push(new OptionRow({
      id: "viewpoint",
      kind: "choice",
      label: say(strings, "ifs.gameopt.camera", "VIEWPOINT"),
      choices: [[true, say(strings, "ifs.GameOpt.cam_cockpit", "1ST PERSON")], [false, say(strings, "ifs.GameOpt.cam_forward", "3RD PERSON")]],
      get: () => Boolean(viewpoint.get()),
      set: (value) => viewpoint.set(Boolean(value)),
    }));
  }
  for (const entry of optionsFor("game")) rows.push(optionRow(entry, settings, { strings, inBattle }));
  rows.push(new OptionRow({ id: "reset", kind: "action", label: "", display: say(strings, "common.reset", "RESET TO DEFAULTS"), action: () => resetPage("game", settings) }));
  return new MenuScreen({
    id: "options-game",
    dim: true,
    widgets: [title("game", strings, 10), new Options({ id: "rows", layout: "game", rows }), backButton(strings)],
  });
}

// VIDEO OPTIONS: every rendering setting, scrolling where they don't fit.
export function videoPage({ settings, strings, inBattle }) {
  const rows = optionsFor("video").map((entry) => optionRow(entry, settings, { strings, inBattle }));
  return new MenuScreen({
    id: "options-video",
    dim: true,
    widgets: [title("video", strings, -30), new Options({ id: "rows", layout: "video", rows }), backButton(strings), resetButton(strings, "video", settings)],
  });
}

// AUDIO OPTIONS: the volumes, the rebuilt audio's own, and the speaker
// test.
export function audioPage({ settings, strings, inBattle, audioPanel }) {
  const rows = optionsFor("audio").map((entry) => optionRow(entry, settings, { strings, inBattle }));
  if (audioPanel) rows.push(new OptionRow({ id: "panel", kind: "action", label: "SPEAKER TEST", display: "AUDIO DETAILS", action: () => audioPanel() }));
  return new MenuScreen({
    id: "options-audio",
    dim: true,
    widgets: [title("audio", strings, 0), new Options({ id: "rows", layout: "audio", rows }), backButton(strings), resetButton(strings, "audio", settings)],
  });
}

// CONTROL OPTIONS: the original's ifs_opt_pccontrols. Up and Down run from
// the two rows into the bindings list and back; Left and Right in the list
// change the tab, as does a click on one. Enter or a double click on a bound
// row asks for its new key (keyPopup), saved at once: the INFANTRY and
// VEHICLE tabs' in keyBindings, the STARFIGHTER tab's in flightBindings.
export function controlsPage({ settings, strings, inBattle }) {
  const flying = () => TABS[tabs.active].id === "starfighter";
  // The rows over the list: the mouse's, or the starfighter's. The bindings
  // are the list itself.
  const rows = () => optionsFor("controls")
    .filter((e) => (flying() ? ["flightSensitivity", "flightInvertY", "flightResponseCurve"].includes(e.key) : !e.key.startsWith("flight") && e.key !== "keyBindings"))
    .map((entry) => optionRow(entry, settings, { strings, inBattle }));
  const bindings = () => hotkeyRows(TABS[tabs.active].id, { strings, bindings: playerBindings(settings.get("keyBindings")), flight: settings.get("flightBindings") });
  const assign = (binding, code) => (flying()
    ? settings.set("flightBindings", rebind(FLIGHT_BINDINGS, settings.get("flightBindings"), binding, code))
    : settings.set("keyBindings", rebind(flatBindings(), settings.get("keyBindings"), binding, code)));
  const tabs = new Tabs({
    id: "tabs",
    tabs: TABS.map((tab) => ({ id: tab.id, label: say(strings, tab.labelKey, tab.label) })),
    look: CONTROLS_TABS,
    onChange: () => {
      options.rows = rows();
      list.selected = 0;
      list.first = 0;
    },
  });
  const options = new Options({ id: "rows", layout: "controls", rows: rows() });
  const list = new List({
    id: "bindings",
    look: surface => bindingsLook(surface, flying() ? 24.5 : 0),
    rows: () => bindings().map((row) => [row.action, row.keys]),
    onAccept: (index, screen) => {
      const row = bindings()[index];
      if (!row?.binding) return;
      screen.stack.push(keyPopup({ strings, action: row.action.replace(/:$/, ""), accepts: bindable, onKey: (code) => assign(row.binding, code) }));
    },
    key(code, item, screen) {
      if (code === "ArrowLeft" || code === "ArrowRight") {
        tabs.choose(tabs.active + (code === "ArrowLeft" ? -1 : 1), screen);
        return true;
      }
      return List.prototype.key.call(this, code, item, screen);
    },
  });
  return new MenuScreen({
    id: "options-controls",
    dim: true,
    widgets: [
      title("controls", strings, 5),
      new Panel({ id: "window", at: [0.5, 0], box: controlsWindow, alpha: 1 }),
      tabs,
      options,
      new Text({ id: "heading", look: "heading", text: say(strings, "ifs.GameOpt.pc_configbindings", "CONFIG BINDINGS"), y: 186, flashy: false }),
      list,
      backButton(strings),
      resetButton(strings, "controls", settings),
    ],
  });
}

// The controls page's frames, in the safe area's units (the original's
// ifs_opt_pccontrols): the big window from the
// tabs down past the screen's bottom, behind the bottom buttons too; the
// tabs along its top; the bindings list from 220 units down, 0.95 of the
// safe height less 240 tall, the safe width wide.
const SAFE_W = 0.9 * 1024;
function controlsWindow(surface) {
  const top = 89;
  const bottom = surface.height;
  return { x: 0, y: (top + bottom) / 2, w: SAFE_W + 42, h: bottom - top };
}
const CONTROLS_TABS = { at: [0, 0], x: 16, y: 79, width: 0.30, spacing: 1 / 3, font: "gamefont_tiny" };
function bindingsLook(surface, extra = 0) {
  const h = 0.9 * surface.height;
  const height = 0.95 * h - 240 - extra;
  return {
    at: [0.5, 0],
    box: { x: 0, y: 220 + extra + height / 2, w: SAFE_W, h: height },
    font: "gamefont_tiny",
    pitch: 22.6,
    inset: 14,
    padLeft: 16,
    padRight: 20,
    columns: [{ x: 0 }, { x: 0.4 }],
  };
}
