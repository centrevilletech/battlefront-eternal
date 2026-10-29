import test from "node:test";
import assert from "node:assert/strict";
import { SpawnModel } from "../src/spawn-screen/model.js";
import { SpawnInput, TAB_REPEAT, bindDom } from "../src/spawn-screen/input.js";
import { FakeSpawnPort, geonosisData } from "./spawn-screen-fake-port.js";

const FRAME = 1 / 60;

function setup({ hits = {}, human = {} } = {}) {
  const port = new FakeSpawnPort({ ...geonosisData(), human });
  const model = new SpawnModel({ port, random: () => 0 });
  const spawned = [];
  const input = new SpawnInput({
    model,
    hit: (x, y) => hits[`${x},${y}`] ?? null,
    spawn: () => spawned.push(model.commit()),
    height: () => 768,
  });
  model.open();
  input.reset();
  return { port, model, input, spawned };
}

test("Down and Up step the class once per press, wrapping", () => {
  const { model, input } = setup();
  input.key("ArrowDown", true);
  input.update(FRAME);
  assert.equal(model.classIndex, 1);
  input.key("ArrowDown", false);
  input.update(FRAME);
  input.key("ArrowUp", true);
  input.update(FRAME);
  assert.equal(model.classIndex, 0);
  input.update(FRAME);
  input.key("ArrowUp", true, true);
  input.update(FRAME);
  assert.equal(model.classIndex, 4, "Up from the first wraps to the last");
});

test("the frame after a class or team key ignores those keys", () => {
  const { model, input } = setup();
  input.key("ArrowDown", true);
  input.update(FRAME);
  input.key("ArrowDown", true, true);
  input.update(FRAME);
  assert.equal(model.classIndex, 1, "the auto-repeat right after is dropped");
  input.key("ArrowDown", true, true);
  input.update(FRAME);
  assert.equal(model.classIndex, 1, "and so is one after a frame that pressed, even dropped");
  input.update(FRAME);
  input.key("ArrowDown", true, true);
  input.update(FRAME);
  assert.equal(model.classIndex, 2);
});

test("the wheel steps the class: up is previous, down is next", () => {
  const { model, input } = setup();
  input.wheel(120);
  input.update(FRAME);
  assert.equal(model.classIndex, 1);
  input.update(FRAME);
  input.wheel(-120);
  input.update(FRAME);
  assert.equal(model.classIndex, 0);
});

test("Left or Right switches team and resets the class", () => {
  const { model, input } = setup();
  input.key("ArrowDown", true);
  input.update(FRAME);
  input.update(FRAME);
  input.key("ArrowLeft", true);
  input.update(FRAME);
  assert.equal(model.team, 2);
  assert.equal(model.classIndex, 0);
});

test("Tab picks the next post at once and again every 0.2 s while held", () => {
  const { model, input } = setup();
  const seen = [];
  model.on("post", ({ post }) => seen.push(post));
  input.key("Tab", true);
  input.update(FRAME);
  assert.equal(seen.length, 0, "the counter starts on the post already picked");
  assert.equal(model.post, "cp10");
  let time = FRAME;
  while (time < TAB_REPEAT + 2 * FRAME) {
    input.update(FRAME);
    time += FRAME;
  }
  assert.deepEqual(seen, ["cp2"]);
  input.key("Tab", false);
  for (let i = 0; i < 30; i++) input.update(FRAME);
  assert.deepEqual(seen, ["cp2"]);
});

test("Space held spawns, even held from before the screen opened", () => {
  const port = new FakeSpawnPort({ ...geonosisData(), human: { phase: "alive" } });
  const model = new SpawnModel({ port, random: () => 0 });
  const spawned = [];
  const input = new SpawnInput({ model, spawn: () => spawned.push(model.commit()), height: () => 768 });
  input.key("Space", true, false, false);
  model.open();
  input.reset();
  input.update(FRAME);
  assert.equal(spawned.length, 1);
  assert.deepEqual(spawned[0], { team: 1, className: "rep_inf_clone_trooper", post: "cp10" });
});

test("the right button spawns; a left click goes to what it lands on", () => {
  const { model, input, spawned } = setup({ hits: { "100,200": { kind: "class", index: 3 }, "512,124": { kind: "team" } } });
  input.move(100, 200);
  input.button(0, true);
  input.update(FRAME);
  assert.equal(model.classIndex, 3);
  input.move(512, 124);
  input.button(0, true);
  input.update(FRAME);
  assert.equal(model.team, 2);
  input.button(2, true);
  assert.equal(spawned.length, 1);
  assert.equal(spawned[0].className, "cis_inf_battledroid");
});

test("Enter clicks where the cursor is; from the centre it does nothing", () => {
  const { model, input, spawned } = setup({ hits: { "512,697": { kind: "spawn" } } });
  input.key("Enter", true);
  input.update(FRAME);
  assert.equal(spawned.length, 0);
  assert.deepEqual(input.cursor, { x: 512, y: 384 });
  input.move(512, 697);
  input.key("Enter", true);
  assert.equal(spawned.length, 1, "SPAWN spawns inside the event");
  assert.equal(model.state, "committed");
});

test("keys while the screen can't act are only tracked", () => {
  const { model, input } = setup();
  assert.equal(input.key("ArrowDown", true, false, false), false);
  input.update(FRAME);
  assert.equal(model.classIndex, 0);
  assert.ok(input.held.has("ArrowDown"));
});

// A window stand-in: listeners by type, and events fired at them.
class Target {
  constructor() {
    this.listeners = new Map();
  }
  addEventListener(type, fn, options) {
    const capture = options === true || options?.capture === true;
    const list = this.listeners.get(type) || [];
    list.push({ fn, capture });
    this.listeners.set(type, list);
  }
  removeEventListener(type, fn) {
    this.listeners.set(type, (this.listeners.get(type) || []).filter((entry) => entry.fn !== fn));
  }
  // Capture listeners first, then the rest, in the order added.
  fire(type, event) {
    const list = this.listeners.get(type) || [];
    for (const entry of list.filter((e) => e.capture)) entry.fn(event);
    for (const entry of list.filter((e) => !e.capture)) entry.fn(event);
  }
}

test("Esc opens the menu over the screen, and the Esc that closes it doesn't reopen it", () => {
  const { input } = setup();
  const window = new Target();
  const element = new Target();
  element.getBoundingClientRect = () => ({ left: 0, top: 0 });
  const menu = { isOpen: false };
  // The menu's own listener comes first and closes it on Esc.
  window.addEventListener("keydown", (event) => {
    if (event.code === "Escape" && menu.isOpen) menu.isOpen = false;
  });
  let opened = 0;
  const unbind = bindDom({
    input, element, target: window, toInterface: (x, y) => ({ x, y }), isOpen: () => true,
    menuOpen: () => menu.isOpen, openMenu: () => { opened += 1; menu.isOpen = true; },
  });
  const key = (code) => ({ code, repeat: false, preventDefault() {} });
  window.fire("keydown", key("Escape"));
  assert.equal(opened, 1);
  assert.equal(menu.isOpen, true);
  window.fire("keydown", key("Escape"));
  assert.equal(menu.isOpen, false);
  assert.equal(opened, 1);
  unbind();
  assert.equal(window.listeners.get("keydown").length, 1);
});
