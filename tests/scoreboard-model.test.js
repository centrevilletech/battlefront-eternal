import test from "node:test";
import assert from "node:assert/strict";
import { ScoreboardModel, compareRows, KILLS, DEATHS, POINTS, NAME, CLOSED, PEEK, END, IDLE_QUIT } from "../src/scoreboard/model.js";
import { captureRows } from "./scoreboard-capture-fixture.js";

const row = (team, name, kills, deaths, points, extra = {}) => ({ team, name, kills, deaths, points, human: false, local: false, ...extra });

// A port with fixed rows.
function port(rows, { team = 2, teams = { 1: { name: "REPUBLIC", reinforcements: 0 }, 2: { name: "CIS", reinforcements: 89 } } } = {}) {
  return { roster: () => rows.map((r, slot) => ({ slot, ...r })), humanTeam: () => team, teams: () => teams };
}

// A deterministic shuffle (the rows' order must not matter).
function shuffled(list, seed = 7) {
  const out = list.slice();
  let s = seed;
  for (let i = out.length - 1; i > 0; i--) {
    s = (s * 1103515245 + 12345) & 0x7fffffff;
    const j = s % (i + 1);
    [out[i], out[j]] = [out[j], out[i]];
  }
  return out;
}

test("the default order: kills most first, then deaths fewest, CP points most, names Z to A", () => {
  const rows = [row(1, "ALPHA 1", 2, 3, 0), row(1, "BETA 2", 2, 3, 1), row(1, "GAMMA 3", 2, 1, 0), row(1, "DELTA 4", 5, 9, 0), row(1, "ZETA 5", 2, 3, 0)];
  const names = rows.slice().sort(compareRows(KILLS)).map((r) => r.name);
  assert.deepEqual(names, ["DELTA 4", "GAMMA 3", "BETA 2", "ZETA 5", "ALPHA 1"]);
});

test("each mode's tie-breaks, and reverse turns only the first key around", () => {
  const rows = [row(1, "A", 1, 2, 3), row(1, "B", 3, 2, 1), row(1, "C", 3, 1, 1), row(1, "D", 0, 2, 3)];
  const order = (mode, reverse = false) => rows.slice().sort(compareRows(mode, reverse)).map((r) => r.name).join("");
  assert.equal(order(DEATHS), "CBAD", "deaths fewest first, then kills");
  assert.equal(order(POINTS), "ADCB", "points, then kills, then deaths");
  assert.equal(order(NAME), "DCBA", "names from Z");
  assert.equal(order(KILLS, true), "DACB", "kills fewest first; deaths still fewest first");
  assert.equal(order(DEATHS, true), "BADC", "deaths most first; kills still most first");
});

test("names compare by UTF-16 code unit, descending, as the original game's negated compare does", () => {
  const rows = [row(1, "UNIT 032", 0, 4, 1), row(1, "UNIT 729", 0, 4, 1), row(1, "Player 1", 0, 4, 1), row(1, "unit 5", 0, 4, 1)];
  assert.deepEqual(rows.slice().sort(compareRows()).map((r) => r.name), ["unit 5", "UNIT 729", "UNIT 032", "Player 1"]);
});

test("both retail captures' tables come out in the captures' order from shuffled rows", () => {
  for (const which of ["geo1", "nab1"]) {
    const rows = captureRows(which);
    const model = new ScoreboardModel({ port: port(shuffled(rows)) });
    model.open(END);
    for (const team of [1, 2]) {
      const expected = rows.filter((r) => r.team === team).map((r) => r.name);
      assert.deepEqual(model.lists[team].map((r) => r.name), expected, `${which} team ${team}`);
    }
  }
});

test("opening puts the selection on the player's row, five rows below the top, the scroll clamped", () => {
  const geo = new ScoreboardModel({ port: port(captureRows("geo1")) });
  geo.open(END);
  assert.equal(geo.state, END);
  assert.deepEqual([geo.side, geo.selected], [2, 24], "PLAYER 1, the 25th CIS row");
  assert.equal(geo.first, 14, "max(24 - 5, 0) = 19, clamped to 30 - 16 rows");
  assert.equal(geo.row(2, geo.selected).local, true);
  assert.equal(geo.canScrollUp, true);
  assert.equal(geo.canScrollDown, false, "the capture shows no down arrows");
  const nab = new ScoreboardModel({ port: port(captureRows("nab1")) });
  nab.open(END);
  assert.deepEqual([nab.side, nab.selected, nab.first], [2, 15, 1]);
  // A short list: no scroll.
  const small = new ScoreboardModel({ port: port([row(1, "A", 0, 0, 0), row(2, "P", 0, 0, 0, { human: true, local: true })]) });
  small.open(PEEK);
  assert.deepEqual([small.side, small.selected, small.first, small.canScrollUp, small.canScrollDown], [2, 0, 0, false, false]);
  // In the middle of a long list: five rows above.
  const many = Array.from({ length: 40 }, (_, i) => row(1, `B ${String(i).padStart(2, "0")}`, 40 - i, 0, 0, i === 20 ? { local: true, human: true } : {}));
  const mid = new ScoreboardModel({ port: port(many, { team: 1 }) });
  mid.open(PEEK);
  assert.deepEqual([mid.selected, mid.first], [20, 15]);
});

test("up, down and pages move within the list, keeping the selection in view", () => {
  const rows = Array.from({ length: 40 }, (_, i) => row(1, `B ${String(i).padStart(2, "0")}`, 40 - i, 0, 0, i === 0 ? { local: true } : {}));
  const model = new ScoreboardModel({ port: port(rows, { team: 1 }) });
  model.open(PEEK);
  assert.deepEqual([model.selected, model.first], [0, 0]);
  assert.equal(model.move(-1), false, "the top: nothing above");
  model.move(1);
  assert.equal(model.selected, 1);
  model.turnPage(1);
  assert.deepEqual([model.selected, model.first], [15, 0], "to the bottom of the view first");
  model.turnPage(1);
  assert.deepEqual([model.selected, model.first], [31, 16], "then a whole page");
  model.turnPage(1);
  assert.deepEqual([model.selected, model.first], [39, 24], "held to the last row");
  model.turnPage(-1);
  assert.deepEqual([model.selected, model.first], [24, 24], "up: to the top of the view first");
  model.turnPage(-1);
  assert.deepEqual([model.selected, model.first], [8, 8]);
});

test("left and right take the same row on the other list, held to its length", () => {
  const rows = [...Array.from({ length: 10 }, (_, i) => row(1, `L${i}`, 10 - i, 0, 0)), ...Array.from({ length: 4 }, (_, i) => row(2, `R${i}`, 10 - i, 0, 0, i === 3 ? { local: true } : {}))];
  const model = new ScoreboardModel({ port: port(rows) });
  model.open(END);
  assert.deepEqual([model.side, model.selected], [2, 3]);
  model.flip();
  assert.deepEqual([model.side, model.selected], [1, 3]);
  model.move(1);
  assert.equal(model.selected, 4, "one row at a time");
  model.select(1, 8);
  model.flip();
  assert.deepEqual([model.side, model.selected], [2, 3], "the right list's last row");
});

test("the arrows scroll a row, dragging the selection along to stay in view", () => {
  const rows = Array.from({ length: 30 }, (_, i) => row(1, `B${String(i).padStart(2, "0")}`, 30 - i, 0, 0, i === 0 ? { local: true } : {}));
  const model = new ScoreboardModel({ port: port(rows, { team: 1 }) });
  model.open(END);
  assert.equal(model.scroll(-1), false);
  model.scroll(1);
  assert.deepEqual([model.first, model.selected], [1, 1]);
  for (let i = 0; i < 20; i++) model.scroll(1);
  assert.equal(model.first, 14, "no further than the last page");
  assert.equal(model.canScrollDown, false);
  assert.equal(model.visible(1).length, 16);
  assert.equal(model.visible(2).every((v) => v.row === null), true, "a team with no rows: blanks");
});

test("a header click sorts by it; the same header again turns it around, a third time back", () => {
  const rows = [row(1, "A", 1, 5, 0, { local: true }), row(1, "B", 3, 1, 2), row(1, "C", 2, 3, 1)];
  const model = new ScoreboardModel({ port: port(rows, { team: 1 }) });
  const events = [];
  model.on("sort", (e) => events.push(e));
  model.open(PEEK);
  assert.deepEqual(model.lists[1].map((r) => r.name), ["B", "C", "A"]);
  model.sortBy(DEATHS);
  assert.deepEqual(model.lists[1].map((r) => r.name), ["B", "C", "A"]);
  model.sortBy(DEATHS);
  assert.deepEqual(model.lists[1].map((r) => r.name), ["A", "C", "B"]);
  model.sortBy(DEATHS);
  assert.deepEqual(model.lists[1].map((r) => r.name), ["B", "C", "A"]);
  model.sortBy(NAME);
  assert.deepEqual(model.lists[1].map((r) => r.name), ["C", "B", "A"]);
  assert.deepEqual(events.map((e) => [e.mode, e.reverse]), [[DEATHS, false], [DEATHS, true], [DEATHS, false], [NAME, false]]);
  // The mode stays for the session.
  model.close();
  model.open(PEEK);
  assert.equal(model.mode, NAME);
});

test("the end table quits after 30 s without input, a frame taking at most half a second", () => {
  const model = new ScoreboardModel({ port: port([row(1, "A", 0, 0, 0, { local: true })], { team: 1 }) });
  let quits = 0;
  model.on("quit", () => quits++);
  model.open(END);
  for (let i = 0; i < 59; i++) model.update(1);
  assert.equal(quits, 0, "59 one-second frames count as 29.5 s");
  model.update(1);
  model.update(0.1);
  assert.equal(quits, 1);
  model.touch();
  assert.equal(model.idle, IDLE_QUIT);
  // Tab's table never quits by itself.
  model.close();
  model.open(PEEK);
  for (let i = 0; i < 100; i++) model.update(1);
  assert.equal(quits, 1);
  model.close();
  assert.equal(model.state, CLOSED);
});

test("Tab in a network game is the scores table too: every Character, bots and players alike, in the table's order", () => {
  // A network game's rows: the players carry their ping, the bots none.
  const rows = [
    row(1, "BOT 1", 9, 0, 0),
    row(2, "JOSH", 3, 4, 1, { human: true, local: true, ping: 31 }),
    row(1, "AMY", 5, 2, 0, { human: true, ping: 120 }),
    row(2, "BOT 2", 1, 0, 0),
    row(1, "ZED", 5, 2, 2, { human: true, ping: null }),
  ];
  const model = new ScoreboardModel({ port: port(rows) });
  assert.equal(model.open(PEEK), true);
  assert.deepEqual(model.lists[1].map((r) => r.name), ["BOT 1", "ZED", "AMY"], "kills, then deaths, then CP points");
  assert.deepEqual(model.lists[2].map((r) => [r.name, r.ping]), [["JOSH", 31], ["BOT 2", undefined]]);
  assert.deepEqual({ side: model.side, selected: model.selected }, { side: 2, selected: 0 }, "the selection starts on the player");
  assert.equal(model.sortBy(DEATHS), true, "the columns sort as offline");
  assert.deepEqual(model.lists[1].map((r) => r.name), ["BOT 1", "ZED", "AMY"]);
  assert.equal(model.sortBy(DEATHS), true);
  assert.deepEqual(model.lists[1].map((r) => r.name), ["ZED", "AMY", "BOT 1"], "most deaths first");
  model.close();
  // The end table lists the same rows, in the order Tab left.
  assert.equal(model.open(END), true);
  assert.deepEqual(model.lists[1].map((r) => r.name), ["ZED", "AMY", "BOT 1"]);
});
