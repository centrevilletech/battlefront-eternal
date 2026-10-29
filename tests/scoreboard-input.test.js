import test from "node:test";
import assert from "node:assert/strict";
import { TabHold, ScoreboardInput, HOLD, DOUBLE_CLICK } from "../src/scoreboard/input.js";
import { ScoreboardModel, END, PEEK, KILLS, DEATHS } from "../src/scoreboard/model.js";

// Frames of 1/60 s with Tab held or not; the first frame that closes it.
function play(hold, frames) {
  let now = 0;
  for (let i = 0; i < frames.length; i++) {
    now += 1 / 60;
    if (hold.step(frames[i], now)) return i;
  }
  return -1;
}
const held = (n) => Array(n).fill(true);
const free = (n) => Array(n).fill(false);

test("held half a second or more, the list closes on the first frame Tab is up", () => {
  const hold = new TabHold();
  hold.start(0);
  assert.equal(play(hold, [...held(40), ...free(1)]), 40);
  hold.start(0);
  assert.equal(play(hold, [...held(29), true, ...free(5)]), 30, "0.5 s exactly counts as held");
});

test("a tap pins it; the next press closes it when that is let go", () => {
  const hold = new TabHold();
  hold.start(0);
  assert.equal(play(hold, [...held(10), ...free(200)]), -1, "let go in 0.17 s: stays up");
  assert.equal(hold.tapped, true);
  // Pressed again a while later: held now, so it closes on the release.
  hold.start(0);
  assert.equal(play(hold, [...held(5), ...free(100), ...held(20), ...free(1)]), 125);
});

test("pressed again within the first half second, it still closes on the release after it", () => {
  const hold = new TabHold();
  hold.start(0);
  // Tapped at 0.08 s, held again from 0.17 s past 0.5 s: held mode again.
  assert.equal(play(hold, [...held(5), ...free(5), ...held(30), ...free(1)]), 40);
  // Tapped, then held from 0.17 s but let go at 0.33 s: still pinned.
  hold.start(0);
  assert.equal(play(hold, [...held(5), ...free(5), ...held(10), ...free(100)]), -1);
});

test("HOLD is the original game's half second", () => {
  assert.equal(HOLD, 0.5);
  assert.equal(DOUBLE_CLICK, 0.4);
});

function board(rows = 20) {
  const list = Array.from({ length: rows }, (_, i) => ({ team: 1, name: `B${String(i).padStart(2, "0")}`, kills: rows - i, deaths: 0, points: 0, human: i === 0, local: i === 0 }));
  const port = { roster: () => list.map((r, slot) => ({ slot, ...r })), humanTeam: () => 1, teams: () => ({}) };
  const model = new ScoreboardModel({ port });
  const acts = [];
  let target = null;
  const input = new ScoreboardInput({ model, hit: () => target, act: (kind, t) => acts.push([kind, t?.index ?? null]) });
  return { model, input, acts, aim: (t) => { target = t; } };
}

test("keys while it's up: Up and Down, Page Up and Down, Left and Right, applied once a frame", () => {
  const { model, input } = board();
  model.open(PEEK);
  assert.equal(input.key("ArrowDown", true), true, "swallowed");
  input.key("ArrowDown", false);
  input.key("ArrowDown", true);
  assert.equal(model.selected, 0, "not before the frame");
  input.update(1 / 60);
  assert.equal(model.selected, 2);
  input.key("PageDown", true);
  input.update(1 / 60);
  assert.equal(model.selected, 15);
  input.key("ArrowRight", true);
  input.update(1 / 60);
  assert.equal(model.side, 1, "the other list is empty: no flip");
  assert.equal(input.key("KeyW", true), false, "other keys go by");
});

test("closed, it takes no keys but keeps track of what is held", () => {
  const { model, input } = board();
  assert.equal(input.key("Tab", true), false);
  assert.equal(input.held.has("Tab"), true);
  input.key("ArrowDown", true);
  input.update(1 / 60);
  assert.equal(model.isOpen, false);
  input.key("Tab", false);
  assert.equal(input.held.has("Tab"), false);
});

test("clicks: a header sorts, an arrow scrolls, a row selects; buttons act from inside the click", () => {
  const { model, input, acts, aim } = board(30);
  model.open(END);
  aim({ kind: "sort", mode: DEATHS });
  input.button(0, true);
  input.update(1 / 60);
  assert.equal(model.mode, DEATHS);
  aim({ kind: "scroll", step: 1 });
  input.button(0, true);
  input.update(1 / 60);
  assert.equal(model.first, 1);
  aim({ kind: "row", side: 1, index: 7 });
  input.button(0, true);
  input.update(1 / 60);
  assert.equal(model.selected, 7);
  assert.deepEqual(model.hover, { side: 1, index: 7 }, "the row under the pointer is lit");
  aim({ kind: "done" });
  input.button(0, true);
  assert.deepEqual(acts.at(-1), ["done", null], "at once");
  input.button(2, true);
  assert.equal(acts.length, 1, "only the left button clicks");
  model.sortBy(KILLS);
});

test("a second click on the same row within 0.4 s opens DETAILS", () => {
  const { model, input, acts, aim } = board();
  model.open(END);
  aim({ kind: "row", side: 1, index: 3 });
  input.click(0, 0);
  input.update(0.2);
  input.click(0, 0);
  assert.deepEqual(acts, [["details", 3]]);
  input.update(0.5);
  input.click(0, 0);
  input.update(0.5);
  input.click(0, 0);
  assert.equal(acts.length, 1, "too slow");
  aim({ kind: "row", side: 1, index: 4 });
  input.click(0, 0);
  assert.equal(acts.length, 1, "another row");
});

test("Enter clicks where the pointer is; it starts in the middle", () => {
  const { model, input, aim } = board();
  model.open(END);
  input.reset();
  assert.deepEqual(input.cursor, { x: 512, y: 384 });
  let at = null;
  input.hit = (x, y) => { at = [x, y]; return null; };
  input.key("Enter", true);
  assert.deepEqual(at, [512, 384]);
  input.nudge(-600, 1000);
  assert.deepEqual(input.cursor, { x: 0, y: 768 }, "held on the screen");
  aim(null);
});
