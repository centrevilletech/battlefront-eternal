import test from "node:test";
import assert from "node:assert/strict";
import { createScoreboard, VIEWS } from "../src/scoreboard/index.js";
import { ClassicView } from "../src/scoreboard/classic/view.js";
import { PageView, sortDirection, tabTarget } from "../src/scoreboard/page/view.js";
import { KILLS, DEATHS, POINTS } from "../src/scoreboard/model.js";
import { captureRows } from "./scoreboard-capture-fixture.js";

// The page view's parts that need no page; its elements are checked in the
// browser (docs/scoreboard.md).

function board(view) {
  const hud = {
    renderer: null, height: 768, strings: {}, layer: null,
    fonts: { lineHeight: () => 23, width: (font, text) => String(text).length * 10 },
    texture: () => null, solid: null,
    setLayer(layer) { this.layer = layer; },
    watch() { return () => {}; },
  };
  const port = { roster: () => captureRows("geo1"), humanTeam: () => 2, teams: () => ({}), state: () => ({ over: false, gameOver: false, finished: false }) };
  return { hud, board: createScoreboard({ port, hud, clock: "manual", view }) };
}

test("\"page\" names the page view; without a name the board is Classic", () => {
  assert.equal(VIEWS.page, PageView);
  assert.ok(board(null).board.view instanceof ClassicView);
  assert.ok(board("page").board.view instanceof PageView);
});

test("the page view's HUD layer hides the HUD's displays and the reinforcements", () => {
  const { hud, board: b } = board("page");
  assert.equal(b.peek(), true);
  assert.equal(hud.layer, b.view.layer);
  assert.equal(hud.layer.teams, false);
  b.close();
  assert.equal(hud.layer, null);
});

test("the sorted column's arrow: down while the most come first, up while the fewest do", () => {
  assert.equal(sortDirection(KILLS), "down");
  assert.equal(sortDirection(KILLS, true), "up");
  assert.equal(sortDirection(DEATHS), "up");
  assert.equal(sortDirection(DEATHS, true), "down");
  assert.equal(sortDirection(POINTS), "down");
});

test("a tab is its page's button: the table's is BACK; none on its own page", () => {
  assert.deepEqual(tabTarget("table", "details"), { kind: "back" });
  assert.deepEqual(tabTarget("details", "table"), { kind: "details" });
  assert.deepEqual(tabTarget("awards", "details"), { kind: "awards" });
  assert.equal(tabTarget("awards", "awards"), null);
});
