import test from "node:test";
import assert from "node:assert/strict";
import { BINDINGS, Controls, bindable, flatBindings, playerBindings, rebind } from "../src/input.js";
import { FLIGHT_BINDINGS, flightBindings, flightInput } from "../src/flight/controls.js";
import { CommandControls } from "../src/runtime/commands.js";
import { encodeEntityInput } from "../src/runtime/network-entity-input.js";
import { DEFAULTS } from "../src/settings.js";
import { resetPage } from "../src/menus/options.js";
import { controlsPage } from "../src/menus/option-pages.js";
import { bindMenuInput } from "../src/menus/input.js";
import { ScreenStack } from "../src/menus/stack.js";

// A Settings stand-in: values and the order things were set.
function settingsStub(values = {}) {
  const stored = { ...DEFAULTS, ...values };
  return { sets: [], get: (key) => stored[key], set(key, value) { this.sets.push([key, value]); stored[key] = value; } };
}

// The page's Controls, with the window and document they listen on.
function pageControls(bindings = null) {
  const old = { window: globalThis.window, document: globalThis.document };
  globalThis.window = new EventTarget();
  globalThis.document = new EventTarget();
  try {
    return new Controls(new EventTarget(), { bindings });
  } finally {
    globalThis.window = old.window;
    globalThis.document = old.document;
  }
}

// A key or mouse event as the browser's, cancelable.
const event = (type, fields) => Object.assign(new Event(type, { cancelable: true }), fields);

test("a new key is the binding's only key, and leaves the binding that had it", () => {
  const own = rebind(flatBindings(), {}, "presses.jump", "KeyC");
  assert.deepEqual(own, { "presses.jump": ["KeyC"], "presses.crouch": ["ControlLeft", "ControlRight"] });
  const tables = playerBindings(own);
  assert.deepEqual(tables.presses.jump, ["KeyC"]);
  assert.deepEqual(tables.presses.crouch, ["ControlLeft", "ControlRight"]);
  assert.deepEqual(tables.presses.prone, BINDINGS.presses.prone, "the rest keep the tables' keys");
  // An axis side, and back to the defaults: nothing is kept.
  const moved = rebind(flatBindings(), own, "axes.forward+", "KeyI");
  assert.deepEqual(playerBindings(moved).axes.forward, [["KeyI"], ["KeyS", "ArrowDown"]]);
  assert.deepEqual(rebind(flatBindings(), { "presses.jump": ["KeyV"] }, "presses.jump", "Space"), {});
  // The starfighter's set the same way.
  assert.deepEqual(rebind(FLIGHT_BINDINGS, {}, "brake", "KeyW"), { brake: ["KeyW"], throttle: [] });
});

test("a stored setting that isn't a list of keys leaves the tables' keys", () => {
  const tables = playerBindings({ "presses.jump": "KeyV", "holds.firePrimary": ["KeyK", 7], "presses.nothing": ["KeyL"] });
  assert.deepEqual(tables.presses.jump, ["Space"]);
  assert.deepEqual(tables.holds.firePrimary, ["KeyK"]);
  assert.equal("nothing" in tables.presses, false);
  assert.deepEqual(playerBindings(null), playerBindings({}));
});

test("the keys a binding can take: the network's, not the ones the game reads itself", () => {
  for (const code of ["KeyV", "F5", "Numpad7", "Mouse4", "WheelUp", "Enter"]) assert.equal(bindable(code), true, code);
  for (const code of ["Escape", "Tab", "KeyT", "KeyY", "F11", "F12", "MetaLeft", "IntlBackslash"]) assert.equal(bindable(code), false, code);
});

test("the page reads presses, holds and axes through the player's keys", () => {
  const controls = pageControls({ "presses.jump": ["KeyV"], "holds.firePrimary": ["KeyK"], "axes.forward+": ["KeyI"] });
  controls.press("KeyV");
  assert.equal(controls.take("jump"), true);
  controls.press("Space");
  assert.equal(controls.take("jump"), false, "Space no longer jumps");
  controls.hold("KeyI", "KeyK");
  assert.equal(controls.axis("forward"), 1);
  assert.equal(controls.held("firePrimary"), true);
  controls.clear();
  // Changed while the battle runs.
  controls.setBindings({});
  controls.press("Space");
  assert.equal(controls.take("jump"), true);
});

test("a command carries the world's keys, so every host reads the player's bindings", () => {
  const controls = pageControls({ "axes.forward+": ["KeyI"], "holds.firePrimary": ["KeyK"], "presses.jump": ["KeyW"], "axes.turn-": ["KeyJ"] });
  controls.hold("KeyI", "KeyK", "KeyW", "KeyJ", "Space", "ControlLeft");
  const command = controls.command(1, 1, 1);
  assert.deepEqual(new Set(command.down), new Set(["KeyW", "Mouse0", "NumpadDivide", "Space", "ControlLeft"]));
  const world = new CommandControls();
  assert.ok(world.accept(command));
  assert.equal(world.axis("forward"), 1, "I walks");
  assert.equal(world.held("firePrimary"), true, "K fires");
  assert.equal(world.axis("turn"), -1, "J turns left");
  assert.equal(world.down.has("Space"), true, "keys the world reads itself go as they are");
  // W is jump's now: held alone it walks nowhere.
  controls.clear();
  controls.hold("KeyW");
  const next = controls.command(2, 1, 1);
  assert.deepEqual(next.down, []);
  assert.ok(world.accept(next));
  assert.equal(world.axis("forward"), 0);
});

test("with the defaults the world's keys hold the same controls (Alt is the second fire)", () => {
  const controls = pageControls();
  controls.hold("ArrowUp", "ArrowLeft", "AltLeft");
  const world = new CommandControls();
  assert.ok(world.accept(controls.command(1, 1, 1)));
  assert.equal(world.axis("forward"), 1);
  assert.equal(world.axis("right"), -1);
  assert.equal(world.held("fireSecondary"), true);
  assert.deepEqual(new Set(world.down), new Set(["KeyW", "KeyA", "Mouse2"]), "the network's fire edges read Mouse2");
});

test("a starfighter's own keys go as its defaults, and the command encodes for the network", () => {
  const custom = { throttle: ["KeyI"], firePrimary: ["KeyK"] };
  const controls = pageControls();
  controls.flightScheme = () => flightBindings(custom);
  controls.hold("KeyI", "KeyK", "KeyW");
  const command = Object.assign(controls.command(1, 1, 1), { lookTotal: [0, 0], flightSettings: { sensitivity: 1, invert: false, curve: 1.35, bindings: {} } });
  assert.deepEqual(new Set(command.down), new Set(["KeyW", "Mouse0"]));
  const world = new CommandControls();
  world.flightScheme = () => flightBindings(command.flightSettings.bindings);
  assert.ok(world.accept(command));
  assert.equal(flightInput(world, flightBindings(command.flightSettings.bindings)).forward, 1);
  assert.equal(world.held("firePrimary"), true);
  assert.doesNotThrow(() => encodeEntityInput({ epoch: 1, generation: 1, commands: [command] }));
  // W held alone was throttle's default key, now no control's.
  controls.clear();
  controls.hold("KeyW");
  assert.deepEqual(controls.command(2, 1, 1).down, []);
});

// The CONTROL page over a stack, focused on the bindings list at `action`.
function controlsAt(action, values = {}) {
  const settings = settingsStub(values);
  const stack = new ScreenStack({ theme: { draw() {}, hit: () => null }, sound: () => {} });
  const page = controlsPage({ settings, strings: {} });
  stack.push(page);
  const list = page.widget("bindings");
  page.focused = { widget: list, item: 0 };
  list.selected = list.rowsNow().findIndex(([name]) => name === `${action}:`);
  assert.ok(list.selected >= 0, action);
  return { settings, stack, page, list };
}

test("Enter on a bound row asks for its key; the key is saved and shown", () => {
  const { settings, stack, page, list } = controlsAt("JUMP");
  stack.key("Enter");
  assert.equal(stack.top.id, "rebind");
  assert.deepEqual(stack.top.widget("message").lines(), ["JUMP", "PRESS NEW KEY, ESC TO CANCEL"]);
  stack.key("KeyT");
  assert.equal(stack.top.id, "rebind", "chat's key is refused; the popup waits on");
  stack.key("KeyV");
  assert.equal(stack.top, page);
  assert.deepEqual(settings.get("keyBindings"), { "presses.jump": ["KeyV"] });
  assert.equal(list.rowsNow()[list.selected][1], "V");
  // Esc: nothing changes.
  stack.key("Enter");
  stack.key("Escape");
  assert.equal(stack.top, page);
  assert.equal(settings.sets.length, 1);
  // A key the game reads itself has no popup.
  list.selected = list.rowsNow().findIndex(([name]) => name === "PAUSE MENU:");
  stack.key("Enter");
  assert.equal(stack.top, page);
});

test("the STARFIGHTER tab's keys go to flightBindings; RESET TO DEFAULTS clears both", () => {
  const { settings, stack, page, list } = controlsAt("PRIMARY FIRE", { keyBindings: { "presses.jump": ["KeyV"] } });
  page.widget("tabs").choose(2, page);
  list.selected = list.rowsNow().findIndex(([name]) => name === "THROTTLE:");
  stack.key("Enter");
  stack.key("KeyI");
  assert.deepEqual(settings.get("flightBindings"), { throttle: ["KeyI"] });
  assert.equal(list.rowsNow()[list.selected][1], "I");
  resetPage("controls", settings);
  assert.deepEqual(settings.get("flightBindings"), {});
  assert.deepEqual(settings.get("keyBindings"), {});
});

test("the popup takes a mouse button or wheel notch ahead of the page, and the rest of that click", () => {
  // The window: its capture listeners (the popup's) come ahead of the
  // page's, here by being added first (an EventTarget has no phases).
  const target = new EventTarget();
  const { settings, stack, list } = controlsAt("ZOOM");
  const open = () => {
    const old = globalThis.window;
    globalThis.window = target;
    try {
      stack.key("Enter");
    } finally {
      globalThis.window = old;
    }
    assert.equal(stack.top.id, "rebind");
  };
  const page = [];
  const listen = (...types) => { for (const type of types) target.addEventListener(type, (e) => page.push(`${type} ${e.button}`)); };
  open();
  listen("mousedown");
  const down = event("mousedown", { button: 3 });
  target.dispatchEvent(down);
  assert.equal(down.defaultPrevented, true);
  listen("mouseup", "click", "auxclick");
  for (const type of ["mouseup", "click", "auxclick"]) target.dispatchEvent(event(type, { button: 3 }));
  assert.deepEqual(page, [], "the page under the popup heard none of that click");
  assert.deepEqual(settings.get("keyBindings"), { "presses.zoom": ["Mouse3"], "presses.perspective": ["KeyQ"] });
  assert.equal(stack.top.id, "options-controls");
  // Closed: the window is the page's again.
  target.dispatchEvent(event("mousedown", { button: 0 }));
  assert.deepEqual(page, ["mousedown 0"]);
  // The wheel.
  open();
  target.dispatchEvent(event("wheel", { deltaY: -120 }));
  assert.deepEqual(playerBindings(settings.get("keyBindings")).presses.zoom, ["WheelUp"]);
  assert.equal(list.rowsNow()[list.selected][1], "MOUSE WHEEL UP");
  // A popup the stack clears (the pause menu closing) stops listening.
  open();
  stack.clear();
  page.length = 0;
  target.dispatchEvent(event("mousedown", { button: 0 }));
  assert.deepEqual(page, ["mousedown 0"]);
});

test("while the popup is open no key reaches the browser (F5 would reload)", () => {
  const { stack } = controlsAt("JUMP");
  const target = new EventTarget();
  const unbind = bindMenuInput({ stack, element: new EventTarget(), toInterface: () => ({ x: 0, y: 0 }), target, pointer: false });
  const f5 = () => event("keydown", { code: "F5", repeat: false });
  const before = f5();
  target.dispatchEvent(before);
  assert.equal(before.defaultPrevented, false);
  target.dispatchEvent(event("keydown", { code: "Enter", repeat: false }));
  assert.equal(stack.top.id, "rebind");
  const during = f5();
  target.dispatchEvent(during);
  assert.equal(during.defaultPrevented, true);
  assert.equal(stack.top.id, "options-controls", "F5 was the key");
  unbind();
});
