import assert from "node:assert/strict";
import test from "node:test";
import { CommandHistory, CommandControls, HUMAN_COMMAND_LIMIT, humanCommands, humanEntry, queueHumanCommand } from "../src/runtime/commands.js";
import { HeadlessControls } from "../src/runtime/headless-ports.js";
import { InputMailbox } from "../src/runtime/transport.js";
import { Unit } from "../src/units.js";

const command = (sequence, extra = {}) => ({ tick: sequence, epoch: 1, generation: 0, sequence, down: [], actions: [], look: [], ...extra });

test("spawn screen choices are cleaned to their values; anything else is refused", () => {
  assert.deepEqual(humanEntry({ kind: "team", team: 2, screen: { open: true } }), { kind: "team", team: 2 });
  assert.deepEqual(humanEntry({ kind: "spawn", className: "rep_inf_clone_trooper", post: "cp2", cursor: [5, 5] }),
    { kind: "spawn", className: "rep_inf_clone_trooper", post: "cp2" });
  assert.deepEqual(humanEntry({ kind: "spawn", className: "rep_inf_clone_trooper" }), { kind: "spawn", className: "rep_inf_clone_trooper", post: null });
  assert.deepEqual(humanEntry({ kind: "suicide", now: true }), { kind: "suicide" });
  assert.deepEqual(humanEntry({ kind: "cancel", why: "back" }), { kind: "cancel" });
  for (const bad of [null, 7, {}, { kind: "team", team: 0 }, { kind: "team", team: 8 }, { kind: "team", team: 1.5 }, { kind: "team", team: "1" },
    { kind: "spawn" }, { kind: "spawn", className: "" }, { kind: "spawn", className: "x".repeat(65) }, { kind: "spawn", className: "a", post: 3 },
    { kind: "spawn", className: "a", post: "" }, { kind: "fly" }]) {
    assert.equal(humanEntry(bad), null, JSON.stringify(bad));
  }
  assert.deepEqual(humanCommands(undefined), []);
  assert.throws(() => humanCommands({ kind: "suicide" }), /invalid human input/);
  assert.throws(() => humanCommands([{ kind: "suicide" }, { kind: "fly" }]), /invalid human input/);
  assert.throws(() => humanCommands(Array.from({ length: HUMAN_COMMAND_LIMIT + 1 }, () => ({ kind: "suicide" }))), /invalid human input/);
});

test("a burst of choices before a command is sent stays within one command", () => {
  const queue = [];
  // Space held over a stalled frame: a request (team, spawn) every repeat.
  for (let i = 0; i < 40; i++) {
    queueHumanCommand(queue, { kind: "team", team: 1 + (i % 2) });
    queueHumanCommand(queue, { kind: "spawn", className: i % 2 ? "rep_inf_jet_trooper" : "cis_inf_droideka", post: `cp${i}` });
  }
  assert.deepEqual(queue, [{ kind: "team", team: 2 }, { kind: "spawn", className: "rep_inf_jet_trooper", post: "cp39" }], "the last of each");
  // The worker takes it.
  const box = new InputMailbox(1);
  assert.equal(box.push(command(1, { human: queue })), true);
  // A suicide keeps its first place (a second does nothing); cancel and
  // spawn keep their last, so the queue does what the whole burst would.
  const mixed = [];
  for (const entry of [{ kind: "spawn", className: "a" }, { kind: "suicide" }, { kind: "team", team: 1 }, { kind: "cancel" },
    { kind: "suicide" }, { kind: "spawn", className: "b" }, { kind: "team", team: 2 }]) queueHumanCommand(mixed, entry);
  assert.deepEqual(mixed.map((entry) => entry.kind === "spawn" ? entry.className : entry.kind === "team" ? entry.team : entry.kind),
    ["suicide", "cancel", "b", 2]);
  assert.ok(mixed.length <= 4);
  // Malformed choices are refused.
  assert.equal(queueHumanCommand(mixed, { kind: "spawn" }), false);
  assert.equal(mixed.length, 4);
});

test("the worker's mailbox validates them and forwards them in order", () => {
  const box = new InputMailbox(1);
  assert.throws(() => box.push(command(1, { human: [{ kind: "spawn" }] })), /invalid human input/);
  assert.equal(box.push(command(1, { human: [{ kind: "team", team: 1, extra: true }] })), true);
  assert.equal(box.push(command(2)), true);
  assert.equal(box.push(command(3, { human: [{ kind: "spawn", className: "cis_inf_droideka", post: "cp3" }] })), true);
  // A late batch of three commands is one tick's input, choices in order.
  const merged = box.take(3, 0);
  assert.deepEqual(merged.human, [{ kind: "team", team: 1 }, { kind: "spawn", className: "cis_inf_droideka", post: "cp3" }]);
  // No choices: no field, as before.
  box.push(command(4));
  assert.equal("human" in box.take(4, 0), false);
  // A previous life's choices go with its stale commands.
  box.push(command(5, { generation: 0, human: [{ kind: "suicide" }] }));
  assert.equal("human" in box.take(5, 1), false);
});

test("the controls send queued choices once, and a cleared input keeps them", () => {
  const controls = new HeadlessControls();
  controls.human.push({ kind: "team", team: 1 }, { kind: "spawn", className: "cis_inf_droideka", post: null });
  controls.clear();
  const first = controls.command(1, 1, 0);
  assert.deepEqual(first.human, [{ kind: "team", team: 1 }, { kind: "spawn", className: "cis_inf_droideka", post: null }]);
  const second = controls.command(2, 1, 0);
  assert.equal("human" in second, false);
  // The history keeps them for replay; the world's controls accept them.
  const history = new CommandHistory();
  history.reset(1);
  assert.equal(history.put(first), true);
  assert.deepEqual(history.get(first.sequence).human, first.human);
  const world = new CommandControls();
  assert.equal(world.accept(first), true);
});

test("the browser's controls send them with the next command", async () => {
  const listeners = [];
  const target = { addEventListener: (...args) => listeners.push(args) };
  globalThis.window ??= target;
  globalThis.document ??= target;
  const { Controls } = await import("../src/input.js");
  const controls = new Controls({ addEventListener() {} });
  controls.human.push({ kind: "suicide" });
  controls.clear();
  const sent = controls.command(1, 1, 0);
  assert.deepEqual(sent.human, [{ kind: "suicide" }]);
  assert.equal("human" in controls.command(2, 1, 0), false);
});

test("a unit takes another body between lives and keeps what it is", () => {
  const soldier = { position: { x: 0 }, velocity: {} };
  const unit = new Unit({ side: "rep", className: "rep_inf_clone_trooper", def: {}, actor: {}, soldier, animator: {}, combat: {}, presentation: {}, human: true });
  unit.id = 1;
  unit.generation = 4;
  unit.team = 2;
  unit.character = { human: true };
  unit.spheresTick = 9;
  const stopped = [];
  unit.jetFlame = { stop: () => stopped.push(true) };
  const droideka = { position: { x: 5 }, velocity: {} };
  unit.setBody({
    side: "cis", className: "cis_inf_droideka", def: { CollisionRootScale: 1.5 }, actor: { a: 1 }, soldier: droideka,
    animator: { b: 1 }, combat: { c: 1 }, presentation: { d: 1 }, walker: true,
  });
  assert.equal(unit.className, "cis_inf_droideka");
  assert.equal(unit.side, "cis");
  assert.equal(unit.soldier, droideka);
  assert.equal(unit.position, droideka.position);
  assert.equal(unit.walker, true);
  assert.equal(unit.kind, 3, "a droideka's entity kind");
  assert.equal(unit.rootScale, 1.5);
  assert.equal(unit.spheresTick, -1, "hit shapes rebuilt for the new body");
  assert.deepEqual(stopped, [true]);
  assert.deepEqual([unit.id, unit.generation, unit.team, unit.human], [1, 4, 2, true]);
  assert.equal(unit.character.human, true);
});
