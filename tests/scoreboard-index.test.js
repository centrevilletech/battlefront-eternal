import test from "node:test";
import assert from "node:assert/strict";
import { createScoreboard } from "../src/scoreboard/index.js";
import { listenSounds } from "../src/audio/events.js";
import { captureRows } from "./scoreboard-capture-fixture.js";

// Just enough of the battle HUD for the board's logic (no drawing).
function hudStub() {
  const watchers = new Set();
  return {
    renderer: null, height: 768, strings: {}, layer: null, banner: { text: "VICTORY" },
    fonts: { lineHeight: () => 23, width: (font, text) => String(text).length * 10 },
    texture: () => null, solid: null,
    setLayer(layer) { this.layer = layer; },
    announce(text) { this.banner = text == null ? null : { text }; },
    watch(fn) { watchers.add(fn); return () => watchers.delete(fn); },
    render() { for (const fn of watchers) fn(this); },
  };
}

function setup({ spawnOpen = false } = {}) {
  const battle = { over: false, gameOver: false, finished: false };
  const port = {
    roster: () => captureRows("geo1"), humanTeam: () => 2, relation: (a, b) => (a === b ? 1 : -1),
    teams: () => ({ 1: { name: "REPUBLIC", reinforcements: 0 }, 2: { name: "CIS", reinforcements: 89 } }),
    state: () => ({ ...battle }), restarts: 0, restart() { this.restarts += 1; },
  };
  const hud = hudStub();
  const controls = { enabled: true, cleared: 0, clear() { this.cleared += 1; } };
  const spawnScreen = { isOpen: spawnOpen };
  const sounds = [];
  const unlisten = listenSounds((type, ...args) => sounds.push([type, ...args]));
  const board = createScoreboard({ port, hud, controls, spawnScreen, clock: "manual" });
  return { battle, port, hud, controls, spawnScreen, sounds, board, unlisten };
}

test("Tab's table: the HUD's layer, the controls off, the game's sound faded; closing gives them back", (t) => {
  const { hud, controls, sounds, board, unlisten } = setup();
  t.after(unlisten);
  assert.equal(board.peek(), true);
  assert.equal(board.state, "peek");
  assert.equal(hud.layer, board.view.layer);
  assert.equal(controls.enabled, false);
  assert.equal(controls.cleared, 1);
  assert.deepEqual(sounds, [["busFade", "ingame", 0, 1]], "silent itself; the game's sound fades out over a second");
  assert.equal(board.peek(), false, "already up");
  board.close();
  assert.equal(hud.layer, null);
  assert.equal(controls.enabled, true);
  assert.deepEqual(sounds.slice(1), [["interface", "shell_menu_exit"], ["busFade", "ingame", 1, 1]]);
});

test("held Tab: closes on the release after half a second; the numbers stay live while up", (t) => {
  const { board, unlisten } = setup();
  t.after(unlisten);
  board.input.key("Tab", true);
  board.peek();
  for (let i = 0; i < 40; i++) board.update(1 / 60);
  assert.equal(board.state, "peek");
  board.input.key("Tab", false);
  board.update(1 / 60);
  assert.equal(board.state, "closed");
});

test("no Tab table while the spawn screen is up, or once the game is over", (t) => {
  const { battle, board, spawnScreen, unlisten } = setup({ spawnOpen: true });
  t.after(unlisten);
  assert.equal(board.peek(), false);
  spawnScreen.isOpen = false;
  assert.equal(board.peek(), true);
  spawnScreen.isOpen = true;
  board.update(1 / 60);
  assert.equal(board.state, "closed", "the spawn screen opening takes it down");
  spawnScreen.isOpen = false;
  board.peek();
  battle.over = true;
  board.update(1 / 60);
  assert.equal(board.state, "peek", "until the game is over, 2 s after the win");
  battle.gameOver = true;
  board.update(1 / 60);
  assert.equal(board.state, "closed");
  assert.equal(board.peek(), false);
});

test("the end table: after the end display, the banner down; DONE, Esc or 30 s idle restart", (t) => {
  const { battle, port, hud, sounds, board, unlisten } = setup();
  t.after(unlisten);
  battle.over = battle.gameOver = true;
  hud.render();
  assert.equal(board.state, "closed", "the end display first");
  battle.finished = true;
  board.update(1 / 60);
  assert.equal(board.state, "end");
  assert.equal(hud.banner, null, "the banner gives way");
  assert.equal(hud.layer, board.view.layer);
  board.input.act("done");
  assert.equal(port.restarts, 1);
  assert.deepEqual(sounds.at(-1), ["interface", "shell_menu_exit"]);
  board.input.act("done");
  assert.equal(port.restarts, 1, "once");
  // Opened again (as the next page would be): 30 s without input.
  board.model.close();
  board.model.open("end");
  for (let i = 0; i < 61; i++) board.update(0.5);
  assert.equal(port.restarts, 2);
});

test("the battle's end takes Tab's table down for the end table, once", (t) => {
  const { battle, board, unlisten } = setup();
  t.after(unlisten);
  board.peek();
  battle.over = true;
  battle.finished = true; // as if the frames between were skipped
  board.update(1 / 60);
  assert.equal(board.state, "end");
  board.model.close();
  board.update(1 / 60);
  assert.equal(board.state, "closed", "not again");
});

test("online Next is neither drawn nor clickable until the next match is ready", (t) => {
  const { battle, port, hud, board, unlisten } = setup();
  t.after(unlisten);
  let ready = false;
  port.canRestart = () => ready;
  battle.finished = true;
  board.update(1 / 60);
  const box = board.view.buttonBox("done", board.view.layout);
  const x = box.x0 + box.width / 2, y = box.top + box.height / 2;
  const labels = [];
  hud.fonts.draw = (batch, matrix, x, y, text) => labels.push(text);
  board.view.drawButtons(hud, { quad() {} }, null, board.view.layout);
  assert.equal(labels.includes(box.text), false);
  assert.equal(board.view.hit(x, y), null);
  board.input.act("done");
  for (let i = 0; i < 61; i++) board.update(0.5);
  assert.equal(port.restarts, 0, "keyboard and idle cannot leave for an unavailable server");
  ready = true;
  labels.length = 0;
  board.view.drawButtons(hud, { quad() {} }, null, board.view.layout);
  assert.equal(labels.includes(box.text), true);
  assert.deepEqual(board.view.hit(x, y), { kind: "done" });
  board.input.act("done");
  assert.equal(port.restarts, 1, "an earlier blocked click cannot latch the board into quitting");
});

test("a declined restart can be retried", (t) => {
  const { battle, port, board, unlisten } = setup();
  t.after(unlisten);
  battle.finished = true;
  board.update(1 / 60);
  let accepted = false;
  port.restart = () => { port.restarts++; return accepted; };
  board.input.act("done");
  accepted = true;
  board.input.act("done");
  board.input.act("done");
  assert.equal(port.restarts, 2);
});

test("DETAILS and AWARDS go straight from one to the other (the page view's tabs), one sound each", (t) => {
  const { battle, sounds, board, unlisten } = setup();
  t.after(unlisten);
  battle.finished = true;
  board.update(1 / 60);
  sounds.length = 0;
  board.input.act("details");
  assert.equal(board.model.page, "details");
  board.input.act("awards");
  assert.equal(board.model.page, "awards");
  board.input.act("details");
  assert.equal(board.model.page, "details");
  board.input.act("details");
  assert.equal(board.model.page, "details", "already there");
  assert.deepEqual(sounds, [["interface", "shell_menu_enter"], ["interface", "shell_menu_enter"], ["interface", "shell_menu_enter"]]);
  board.input.act("back");
  assert.equal(board.model.page, "table");
});

test("sorting, moving and scrolling play shell_select_change", (t) => {
  const { sounds, board, unlisten } = setup();
  t.after(unlisten);
  board.peek();
  sounds.length = 0;
  board.model.sortBy(1);
  board.model.move(-1);
  board.model.scroll(-1);
  assert.deepEqual(sounds, [["interface", "shell_select_change"], ["interface", "shell_select_change"], ["interface", "shell_select_change"]]);
});
