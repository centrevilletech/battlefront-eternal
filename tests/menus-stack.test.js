import test from "node:test";
import assert from "node:assert/strict";
import { ScreenStack, PULSE } from "../src/menus/stack.js";
import { MenuScreen, FADE_IN } from "../src/menus/screen.js";
import { Rows, Options, Button, List, Tabs, HorizontalButtons } from "../src/menus/widgets.js";
import { confirmPopup, okPopup } from "../src/menus/popup.js";
import { rowsShift, rowBars } from "../src/menus/themes/classic/index.js";

// A theme that draws nothing and gives each item a hotspot: item i of
// widget w is the unit square at (w's index, i).
function gridTheme() {
  return {
    draw() {},
    hit(stack, x, y) {
      const screen = stack.top;
      const widget = screen.widgets[Math.floor(x)];
      const item = Math.floor(y);
      if (!widget || !widget.items().includes(item)) return null;
      return { widget, item };
    },
  };
}

function setup() {
  const sounds = [];
  const stack = new ScreenStack({ theme: gridTheme(), sound: (kind) => sounds.push(kind) });
  return { stack, sounds };
}

const rows = (labels, actions = {}) => new Rows({ id: "rows", rows: labels.map((label) => ({ id: label, label, action: actions[label] })) });

test("pushing enters a screen with its first item focused; popping comes back to the one under it with shell_menu_exit", () => {
  const { stack, sounds } = setup();
  const main = new MenuScreen({ id: "main", widgets: [rows(["A", "B", "C"])], focus: "keep" });
  const sub = new MenuScreen({ id: "sub", widgets: [rows(["X", "Y"])] });
  stack.push(main);
  assert.equal(stack.top, main);
  assert.equal(main.focused.item, 0);
  main.key("ArrowDown");
  assert.equal(main.focused.item, 1);
  stack.push(sub);
  assert.equal(stack.top, sub);
  assert.equal(sub.focused.item, 0);
  sub.key("Escape");
  assert.equal(stack.top, main);
  assert.equal(main.focused.item, 1, "focus: keep comes back to the item left from");
  assert.deepEqual(sounds, ["select", "exit"]);
});

test("Up and Down walk the ring and wrap; with nothing focused both go to the first", () => {
  const { stack, sounds } = setup();
  const screen = new MenuScreen({ widgets: [rows(["A", "B", "C"])] });
  stack.push(screen);
  screen.key("ArrowUp");
  assert.equal(screen.focused.item, 2, "Up from the first wraps to the last");
  screen.key("ArrowDown");
  assert.equal(screen.focused.item, 0, "Down from the last wraps to the first");
  screen.focused = null;
  screen.key("ArrowDown");
  assert.equal(screen.focused.item, 0);
  screen.focused = null;
  screen.key("ArrowUp");
  assert.equal(screen.focused.item, 0);
  assert.equal(sounds.filter((s) => s === "select").length, 4);
});

test("hidden and disabled rows are skipped; BACK is off the ring", () => {
  const { stack } = setup();
  const widget = new Rows({ rows: [{ id: "a", label: "A" }, { id: "b", label: "B", hidden: true }, { id: "c", label: "C", disabled: true }, { id: "d", label: "D" }] });
  const back = new Button({ place: "back", text: "BACK" });
  const reset = new Button({ place: "middle", text: "RESET" });
  const screen = new MenuScreen({ widgets: [widget, back, reset] });
  stack.push(screen);
  assert.deepEqual(screen.ring().map((t) => (t.widget === widget ? t.item : t.widget.place)), [0, 3, "middle"]);
});

test("the pointer gives the focus to what is under it, and to nothing over empty space", () => {
  const { stack, sounds } = setup();
  const screen = new MenuScreen({ widgets: [rows(["A", "B", "C"])] });
  stack.push(screen);
  stack.pointerMove(0.5, 2.5);
  assert.equal(screen.focused.item, 2);
  stack.pointerMove(0.6, 2.6);
  assert.equal(sounds.length, 1, "moving within the same item is silent");
  stack.pointerMove(5, 5);
  assert.equal(screen.focused, null, "over empty space nothing is focused");
  screen.key("ArrowDown");
  assert.equal(screen.focused.item, 0, "the next Down starts from the first");
});

test("Enter and a click accept, with shell_menu_accept", () => {
  const { stack, sounds } = setup();
  const chosen = [];
  const screen = new MenuScreen({ widgets: [rows(["A", "B"], { A: () => chosen.push("A"), B: () => chosen.push("B") })] });
  stack.push(screen);
  screen.key("Enter");
  stack.pointerMove(0.5, 1.5);
  stack.pointerDown(0);
  assert.deepEqual(chosen, ["A", "B"]);
  assert.deepEqual(sounds, ["accept", "select", "accept"]);
});

test("a screen's texts play their intro again on every entry, and its items fade in over 0.4 s", () => {
  const { stack } = setup();
  const screen = new MenuScreen({ widgets: [rows(["A"])] });
  stack.push(screen);
  const intro = screen.intro("rows:0", 0.1);
  assert.equal(intro.done, false);
  stack.update(FADE_IN / 2);
  assert.equal(screen.shown, 0.5);
  stack.update(1);
  assert.equal(intro.done, true);
  assert.equal(screen.shown, 1);
  stack.push(new MenuScreen({ widgets: [rows(["X"])] }));
  stack.pop();
  assert.equal(screen.shown, 0);
  assert.equal(screen.intro("rows:0", 0.1).done, false, "coming back plays it again");
});

test("the focus pulse runs 0.3 -> 1 -> 0.3 at 2 a second, frame by frame as the original steps it", () => {
  const { stack } = setup();
  const frames = (n) => {
    for (let i = 0; i < n; i++) stack.update(1 / 60);
  };
  assert.equal(stack.pulse, PULSE.start);
  frames(15);
  assert.ok(Math.abs(stack.pulse - 1) < 1e-9, "at the top after a quarter second");
  frames(22);
  assert.ok(Math.abs(stack.pulse - 0.3) < 0.04, `at the bottom 0.35 s later (${stack.pulse})`);
  frames(12);
  assert.ok(stack.pulse > 0.6 && stack.pulse < 0.75, "and on the way up again");
});

test("the YES / NO popup: NO focused, Left and Right move, NO and Esc cancel, YES accepts; the screen under stays", () => {
  const { stack, sounds } = setup();
  const answers = [];
  const screen = new MenuScreen({ widgets: [rows(["A"])] });
  stack.push(screen);
  const ask = () => stack.push(confirmPopup({ message: "SURE?", onYes: () => answers.push("yes"), onNo: () => answers.push("no") }));
  ask();
  assert.equal(stack.visible.length, 2, "drawn over the screen under it");
  const popup = stack.top;
  const buttons = popup.widget("answer");
  assert.equal(popup.focused.item, 1, "NO has the focus");
  assert.equal(sounds.length, 0, "opening is silent");
  stack.key("Enter");
  assert.deepEqual(answers, ["no"]);
  assert.equal(stack.top, screen);
  assert.deepEqual(sounds, ["cancel"], "back without the screen under it coming up again");
  ask();
  stack.key("Escape");
  assert.deepEqual(answers, ["no", "no"]);
  ask();
  stack.key("ArrowLeft");
  assert.equal(stack.top.focused.item, 0);
  assert.equal(stack.top.focused.widget.buttons[0].id, "yes");
  stack.key("Enter");
  assert.deepEqual(answers, ["no", "no", "yes"]);
  assert.equal(sounds.at(-1), "accept");
  assert.ok(buttons instanceof HorizontalButtons);
});

test("options: Left and Right step choices (wrapping), toggles and sliders; Enter cycles a choice", () => {
  const { stack, sounds } = setup();
  const values = { mode: "b", on: false, level: 0.5 };
  const options = new Options({
    rows: [
      { id: "mode", kind: "choice", label: "MODE", choices: [["a", "A"], ["b", "B"], ["c", "C"]], get: () => values.mode, set: (v) => (values.mode = v) },
      { id: "on", kind: "toggle", label: "ON", get: () => values.on, set: (v) => (values.on = v) },
      { id: "level", kind: "slider", label: "LEVEL", min: 0, max: 1, step: 0.1, get: () => values.level, set: (v) => (values.level = v) },
      { id: "fixed", kind: "fixed", label: "RATE", display: "60 HZ" },
    ],
  });
  const screen = new MenuScreen({ widgets: [options] });
  stack.push(screen);
  assert.deepEqual(screen.ring().map((t) => t.item), [0, 1, 2], "a fixed row isn't focusable");
  screen.key("ArrowRight");
  screen.key("ArrowRight");
  assert.equal(values.mode, "a", "wraps");
  screen.key("ArrowLeft");
  assert.equal(values.mode, "c");
  screen.key("Enter");
  assert.equal(values.mode, "a");
  screen.key("ArrowDown");
  screen.key("ArrowLeft");
  assert.equal(values.on, true);
  screen.key("ArrowDown");
  screen.key("ArrowRight");
  assert.equal(values.level, 0.6);
  for (let i = 0; i < 10; i++) screen.key("ArrowRight");
  assert.equal(values.level, 1, "clamped");
  assert.equal(options.rows[2].text(), "100%");
  assert.equal(options.rows[3].text(), "60 HZ");
  assert.ok(sounds.filter((s) => s === "accept").length >= 6);
});

test("a slider follows the pointer along its track", () => {
  const { stack } = setup();
  let level = 0;
  const options = new Options({ rows: [{ id: "level", kind: "slider", min: 0, max: 10, step: 0.5, get: () => level, set: (v) => (level = v) }] });
  const screen = new MenuScreen({ widgets: [options] });
  stack.push(screen);
  const target = { widget: options, item: 0, track: { x0: 100, x1: 200 } };
  stack.pointer.x = 150;
  options.press(target, screen);
  assert.equal(level, 5);
  options.drag(target, 177, 0, screen);
  assert.equal(level, 7.5, "snapped to its step");
  options.drag(target, 400, 0, screen);
  assert.equal(level, 10);
});

test("an option list that doesn't fit scrolls to show the focus", () => {
  const { stack } = setup();
  const values = Array.from({ length: 10 }, () => false);
  const options = new Options({ rows: values.map((_, i) => ({ id: `r${i}`, kind: "toggle", label: `R${i}`, get: () => values[i], set: (v) => (values[i] = v) })) });
  options.shown = 4;
  const screen = new MenuScreen({ widgets: [options] });
  stack.push(screen);
  for (let i = 0; i < 6; i++) screen.key("ArrowDown");
  assert.equal(screen.focused.item, 6);
  assert.equal(options.first, 3);
  screen.key("ArrowDown");
  screen.key("ArrowDown");
  screen.key("ArrowDown");
  assert.equal(screen.focused.item, 9);
  screen.key("ArrowDown");
  assert.equal(screen.focused.item, 0);
  assert.equal(options.first, 0);
  assert.equal(options.wheel(1), true);
  assert.equal(options.first, 1);
});

test("a list moves its own selection, hands the focus on at its ends, and accepts on a double click", () => {
  const { stack } = setup();
  const accepted = [];
  const list = new List({ rows: [["A"], ["B"], ["C"]], onAccept: (i) => accepted.push(i) });
  const before = rows(["X"]);
  const screen = new MenuScreen({ widgets: [before, list] });
  stack.push(screen);
  screen.key("ArrowDown");
  assert.equal(screen.focused.widget, list);
  screen.key("ArrowDown");
  screen.key("ArrowDown");
  assert.equal(list.selected, 2);
  screen.key("ArrowDown");
  assert.equal(screen.focused.widget, before, "past its end, the ring goes on");
  screen.key("ArrowUp");
  assert.equal(screen.focused.widget, list);
  screen.key("ArrowUp");
  assert.equal(list.selected, 1);
  list.press({ widget: list, item: 0, row: 0 }, screen);
  assert.equal(list.selected, 0);
  stack.update(0.2);
  list.press({ widget: list, item: 0, row: 0 }, screen);
  assert.deepEqual(accepted, [0], "a second click within 0.4 s");
  stack.update(1);
  list.press({ widget: list, item: 0, row: 2 }, screen);
  stack.update(0.5);
  list.press({ widget: list, item: 0, row: 2 }, screen);
  assert.deepEqual(accepted, [0], "too slow for a double click");
  list.wheel(-1, null, screen);
  assert.equal(list.selected, 1);
});

test("tabs step with Left and Right and report the change", () => {
  const { stack } = setup();
  const changes = [];
  const tabs = new Tabs({ tabs: [{ id: "a", label: "A" }, { id: "b", label: "B" }, { id: "c", label: "C" }], onChange: (i) => changes.push(i) });
  const screen = new MenuScreen({ widgets: [tabs] });
  stack.push(screen);
  tabs.step(0, -1, screen);
  tabs.activate(1, screen);
  assert.deepEqual(changes, [2, 1]);
  assert.equal(tabs.active, 1);
});

test("the row stacks are laid out as the original's AddVerticalButtons", () => {
  // The container's shift: half the widest bar of the whole list.
  assert.equal(rowsShift(260, 28, 11), -200, "the pause menu (11 rows, 6 shown)");
  assert.equal(rowsShift(250, 28, 7), -167, "the main menu");
  assert.equal(rowsShift(360, 33, 8), -229.5, "OPTIONS");
  // The visible rows' bars: the arrow.
  assert.deepEqual(rowBars(260, 28, 6).map((b) => [b.width, b.flipped]), [[260, false], [288, false], [316, false], [316, true], [288, true], [260, true]]);
  assert.deepEqual(rowBars(250, 28, 5).map((b) => [b.width, b.flipped]), [[250, false], [278, false], [278, true], [250, true], [222, true]]);
});

test("popping the last screen calls onEmpty", () => {
  let empty = 0;
  const stack = new ScreenStack({ theme: gridTheme(), sound: () => {}, onEmpty: () => (empty += 1) });
  stack.push(new MenuScreen({ widgets: [rows(["A"])] }));
  stack.key("Escape");
  assert.equal(stack.isEmpty, true);
  assert.equal(empty, 1);
});

test("the OK popup: OK or Esc closes it and calls onOk", () => {
  const { stack, sounds } = setup();
  const screen = new MenuScreen({ widgets: [rows(["A"])] });
  stack.push(screen);
  let oks = 0;
  stack.push(okPopup({ message: "SELECTED MAPLIST IS FULL.", onOk: () => (oks += 1) }));
  assert.equal(stack.top.focused.widget.buttons[0].id, "ok");
  stack.key("Enter");
  assert.equal(stack.top, screen);
  stack.push(okPopup({ message: "FULL", onOk: () => (oks += 1) }));
  stack.key("Escape");
  assert.equal(oks, 2);
  assert.deepEqual(sounds, ["accept", "accept"]);
});
