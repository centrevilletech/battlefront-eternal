import test from "node:test";
import assert from "node:assert/strict";
import { createPauseMenu, LAYOUT } from "../src/pause/index.js";
import { withSettings } from "../src/pause/overrides.js";
import { listenSounds } from "../src/audio/events.js";
import { DEFAULTS } from "../src/settings.js";

globalThis.performance ??= { now: () => Date.now() };

// A window that keeps its keydown listeners, to press keys at.
function fakeWindow() {
  const listeners = [];
  return {
    listeners,
    addEventListener(type, fn, options) {
      listeners.push({ type, fn, capture: Boolean(options?.capture ?? options === true) });
    },
    removeEventListener(type, fn) {
      const i = listeners.findIndex((l) => l.type === type && l.fn === fn);
      if (i >= 0) listeners.splice(i, 1);
    },
    press(code, extra = {}) {
      const event = { type: "keydown", code, repeat: false, stopped: false, prevented: false, ...extra,
        stopImmediatePropagation() { this.stopped = true; }, preventDefault() { this.prevented = true; } };
      for (const l of [...listeners.filter((l) => l.type === "keydown" && l.capture), ...listeners.filter((l) => l.type === "keydown" && !l.capture)]) {
        if (event.stopped) break;
        l.fn(event);
      }
      return event;
    },
  };
}

function setup({ owners = [], restart = undefined } = {}) {
  const values = { ...DEFAULTS };
  const settings = { get: (k) => values[k], set: (k, v) => { values[k] = v; } };
  const hud = { overlay: null, height: 768, setOverlay(o) { this.overlay = o; }, toInterface: (x, y) => ({ x, y }) };
  const calls = [];
  const target = fakeWindow();
  const controls = { released: 0, release() { this.released += 1; } };
  const sounds = [];
  const unlisten = listenSounds((type, name) => sounds.push(name));
  const menu = createPauseMenu({
    hud, settings, controls, clock: "manual", target, element: null, restart,
    onOpen: () => calls.push("open"),
    onClose: (resume) => calls.push(`close:${resume}`),
    navigate: (url) => calls.push(`go:${url}`),
    closeWindow: () => calls.push("window.close"),
  });
  menu.setEscapeOwners(...owners);
  return { menu, hud, calls, target, controls, sounds, values, unlisten };
}

const rowsOf = (menu) => menu.screen.widget("pause");
const choose = (menu, id) => {
  const rows = rowsOf(menu);
  menu.stack.top.focused = { widget: rows, item: rows.rows.findIndex((r) => r.id === id) };
  menu.stack.key("Enter");
};

test("Esc opens GAME PAUSED over the battle: the world paused, the HUD covered, the mouse let go", (t) => {
  const { menu, hud, calls, target, controls, unlisten } = setup();
  t.after(unlisten);
  const event = target.press("Escape");
  assert.equal(menu.isOpen, true);
  assert.equal(event.stopped, true, "nothing else sees that Esc");
  assert.deepEqual(calls, ["open"]);
  assert.equal(hud.overlay, menu.overlay);
  assert.equal(hud.overlay.hidesHud, true);
  assert.equal(controls.released, 1, "the game's own release");
  const rows = rowsOf(menu);
  assert.equal(rows.title, "GAME PAUSED");
  assert.deepEqual(rows.visibleRows().map(({ row }) => row.label instanceof Function ? row.label() : row.label),
    ["RESUME GAME", "OPTIONS", "RESPAWN", "RESTART MISSION", "QUIT", "EXIT TO WINDOWS"]);
  assert.equal(menu.stack.top.focused.item, 0, "RESUME GAME focused");
  assert.equal(rows.listCount, 11, "the original list's length places the container");
  assert.deepEqual([LAYOUT.at, LAYOUT.rotY, LAYOUT.width], [[0.6, 0.5], 25, 260]);
});

test("RESUME, BACK and Esc close it and resume", (t) => {
  const { menu, hud, calls, unlisten } = setup();
  t.after(unlisten);
  menu.open();
  choose(menu, "resume");
  assert.equal(menu.isOpen, false);
  assert.equal(hud.overlay, null);
  menu.open();
  menu.stack.key("Escape");
  assert.equal(menu.isOpen, false);
  menu.open();
  menu.screen.widget("back").activate(0, menu.stack.top);
  assert.deepEqual(calls, ["open", "close:true", "open", "close:true", "open", "close:true"]);
});

test("every choice but RESUME and OPTIONS asks first, NO focused; NO comes back to the menu", (t) => {
  const { menu, calls, sounds, unlisten } = setup();
  t.after(unlisten);
  menu.open();
  for (const id of ["suicide", "restart", "quit", "exit"]) {
    choose(menu, id);
    const popup = menu.stack.top;
    assert.equal(popup.popup, true, `${id} asks`);
    assert.equal(popup.focused.item, 1, "NO focused");
    menu.stack.key("Enter");
    assert.equal(menu.stack.top, menu.screen, "NO goes back to the menu");
  }
  assert.deepEqual(calls, ["open"], "nothing done");
  assert.equal(sounds.filter((s) => s === "shell_menu_cancel").length, 4);
  const questions = [];
  for (const id of ["suicide", "restart", "quit", "exit"]) {
    choose(menu, id);
    questions.push(menu.stack.top.widget("message").lines().join(" "));
    menu.stack.key("Escape");
  }
  assert.deepEqual(questions, [
    "DO YOU REALLY WISH TO KILL YOUR CURRENT PLAYER?",
    "DO YOU REALLY WANT TO RESTART THIS BATTLE?",
    "LEAVING THE GAME NOW WILL END THIS SESSION. ARE YOU SURE YOU WANT TO LEAVE?",
    "REALLY EXIT TO WINDOWS?",
  ]);
});

test("RESPAWN's YES kills the soldier and resumes; it does nothing to one who isn't alive", (t) => {
  const { menu, calls, unlisten } = setup();
  t.after(unlisten);
  let alive = true;
  let kills = 0;
  menu.enableRespawn({ label: "RESPAWN", prompt: "KILL?", available: () => alive, respawn: () => (kills += 1) });
  menu.open();
  choose(menu, "suicide");
  assert.equal(menu.stack.top.widget("message").lines()[0], "KILL?");
  menu.stack.key("ArrowLeft");
  menu.stack.key("Enter");
  assert.equal(kills, 1);
  assert.equal(menu.isOpen, false);
  alive = false;
  menu.open();
  choose(menu, "suicide");
  menu.stack.key("ArrowLeft");
  menu.stack.key("Enter");
  assert.equal(kills, 1);
  assert.deepEqual(calls, ["open", "close:true", "open", "close:true"]);
});

test("RESTART MISSION, QUIT and EXIT TO WINDOWS go where they should", (t) => {
  globalThis.location = { href: "http://localhost/?map=geo1&ai=classic&noreload" };
  t.after(() => delete globalThis.location);
  const { menu, calls, values, unlisten } = setup();
  t.after(unlisten);
  values.aiMode = "off";
  menu.open();
  choose(menu, "restart");
  menu.stack.key("ArrowLeft");
  menu.stack.key("Enter");
  assert.equal(calls.at(-1), "go:http://localhost/?map=geo1&ai=off&noreload", "the same battle, with the AI as the options now say");
  menu.close();
  menu.open();
  choose(menu, "quit");
  menu.stack.key("ArrowLeft");
  menu.stack.key("Enter");
  const quit = calls.at(-1);
  assert.ok(quit.startsWith("go:/?") && quit.includes("screen=main") && !quit.includes("map=") && quit.includes("noreload"), `the main menu, dev flags kept (${quit})`);
  menu.close();
  menu.open();
  choose(menu, "exit");
  menu.stack.key("ArrowLeft");
  menu.stack.key("Enter");
  assert.equal(calls.at(-1), "window.close");
});

test("OPTIONS opens the option pages over the battle, with VIEWPOINT", (t) => {
  const { menu, unlisten } = setup();
  t.after(unlisten);
  menu.open();
  choose(menu, "opts");
  assert.equal(menu.stack.top.id, "options");
  menu.stack.key("Enter");
  assert.equal(menu.stack.top.id, "options-game");
  menu.stack.key("Escape");
  menu.stack.key("Escape");
  assert.equal(menu.stack.top, menu.screen, "BACK twice: GAME PAUSED again");
});

test("a screen that takes Esc itself opens it from there; while it is up Esc isn't the menu's", (t) => {
  const board = { isOpen: true };
  const { menu, target, unlisten } = setup({ owners: [board] });
  t.after(unlisten);
  const event = target.press("Escape");
  assert.equal(menu.isOpen, false);
  assert.equal(event.stopped, false, "left for the screen");
  board.isOpen = false;
  target.press("Escape");
  assert.equal(menu.isOpen, true);
});

test("RESUME waiting for the mouse: CLICK TO RESUME over the HUD, until it is captured", (t) => {
  const { menu, hud, unlisten } = setup();
  t.after(unlisten);
  menu.open();
  menu.close(true);
  menu.awaitPointer(true);
  assert.equal(menu.waiting, true);
  assert.equal(hud.overlay, menu.overlay);
  assert.equal(hud.overlay.hidesHud, false, "the HUD shows again");
  menu.awaitPointer(false);
  assert.equal(hud.overlay, null);
  assert.equal(menu.waiting, false);
});

test("RESTART's URL carries the options' changes to the overrides it has, and only those", () => {
  const settings = { get: (k) => ({ aiMode: "classic", aiDifficulty: "easy", aiHeroes: true, renderScale: 1.5 })[k] };
  assert.equal(withSettings("/?map=nab2&ai=off&tier=hard&heroes=0", settings, "http://localhost/"), "http://localhost/?map=nab2&ai=classic&tier=easy&heroes=1");
  assert.equal(withSettings("/?map=nab2", settings, "http://localhost/"), "http://localhost/?map=nab2");
});


test("a second Escape resumes on release, even within the pointer-lock grace interval", (t) => {
  const { menu, target, calls, unlisten } = setup();
  t.after(unlisten);
  const release = () => {
    for (const listener of target.listeners.filter((l) => l.type === "keyup")) listener.fn({ code: "Escape" });
  };
  menu.open("pointer");
  target.press("Escape"); // Browser's original unlock press must not close it.
  assert.equal(menu.isOpen, true);
  release();
  target.press("Escape");
  assert.equal(menu.isOpen, true, "do not relock while Escape remains down");
  release();
  assert.equal(menu.isOpen, false);
  assert.deepEqual(calls, ["open", "close:true"]);
});

test("online a client has no RESTART MISSION, and the hosting machine's row asks its server instead of the page", (t) => {
  const client = setup({ restart: false });
  t.after(client.unlisten);
  client.target.press("Escape");
  assert.deepEqual(rowsOf(client.menu).visibleRows().map(({ row }) => row.id), ["resume", "opts", "suicide", "quit", "exit"]);
  client.menu.close(true);
  let asked = 0;
  const host = setup({ restart: { label: "RESTART MATCH", run: () => { asked += 1; } } });
  t.after(host.unlisten);
  host.target.press("Escape");
  assert.ok(rowsOf(host.menu).visibleRows().some(({ row }) => row.id === "restart" && row.label === "RESTART MATCH"));
  choose(host.menu, "restart");
  host.menu.stack.key("ArrowLeft"); // NO is focused first, as for every question
  host.menu.stack.key("Enter"); // YES
  assert.equal(asked, 1);
  assert.equal(host.menu.isOpen, false, "the menu closes; the server reloads the match for everyone");
  assert.ok(!host.calls.some((call) => call.startsWith("go:")), "no page navigation of its own");
});
