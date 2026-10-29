import test from "node:test";
import assert from "node:assert/strict";
import { scoreboardPort } from "../src/scoreboard/port.js";
import { Character } from "../src/battle.js";

// A network game's battle as the client has it from the roster: two players
// (one of them this page's), bots on both teams, a local, and a dormant
// slot (team 0).
function battle() {
  const make = (team, human, name, stats = {}, link = null) => Object.assign(new Character(0, team, human), { name, link }, stats);
  const me = make(2, true, "Josh", { kills: 4, teamKills: 1, deaths: 2, captures: 3 }, { ping: 31, kind: "direct6" });
  const characters = [
    make(1, false, "DELTA 855", { kills: 6, deaths: 1 }),
    make(1, true, "Amy", { kills: 2, deaths: 5, captures: 1 }, { ping: 120, kind: "relay4" }),
    me,
    make(2, false, "UNIT 538", { kills: 1, deaths: 3 }),
    make(3, false, "GEONOSIAN"),
    make(0, true, "dormant"),
  ];
  return { characters, human: me, teams: { 1: {}, 2: {} }, names: { 1: "REPUBLIC", 2: "CIS" }, reinforcements: { 1: 150, 2: 149.5 } };
}

test("the roster is every Character of both armies, bots included; a network game's players carry their ping", () => {
  const port = scoreboardPort({ battle: battle(), network: true });
  assert.equal(port.network, true);
  assert.deepEqual(port.roster().map((r) => [r.slot, r.team, r.name, r.kills, r.deaths, r.points, r.human, r.local, r.ping]), [
    [0, 1, "DELTA 855", 6, 1, 0, false, false, null],
    [1, 1, "Amy", 2, 5, 1, true, false, 120],
    [2, 2, "Josh", 2, 2, 3, true, true, 31],
    [3, 2, "UNIT 538", 1, 3, 0, false, false, null],
  ], "KILLS is kills - 2 x team kills; locals and dormant slots aren't listed");
  // The players attached, for the player's activity.
  assert.deepEqual(port.players().map((p) => [p.name, p.ping, p.link, p.local]), [["Amy", 120, "relay4", false], ["Josh", 31, "direct6", true]]);
});

test("offline nobody has a ping and there are no players to count", () => {
  const port = scoreboardPort({ battle: battle() });
  assert.equal(port.network, false);
  assert.ok(port.roster().every((r) => r.ping === null));
  assert.equal(port.players(), null);
});

test("a network game's player's DETAILS end with his connection; a bot's and an offline player's don't", () => {
  const online = scoreboardPort({ battle: battle(), network: true });
  const labels = (rows) => rows.map((r) => r.label);
  const personal = ["HEAD SHOTS", "FAVORITE CLASS", "FAVORITE WEAPON", "FAVORITE VEHICLE", "LONGEST LIVING STREAK", "NARROW ESCAPES", "VEHICLES DESTROYED", "NEMESIS", "BAIT"];
  assert.deepEqual(online.personal(1).at(-1), { key: "ifs.MPLobby.qos_header", label: "CONNECTION", value: "RELAY IPV4" });
  assert.deepEqual(online.personal(2).at(-1).value, "DIRECT IPV6");
  assert.deepEqual(labels(online.personal(0)), personal, "a bot has no connection");
  assert.deepEqual(labels(scoreboardPort({ battle: battle() }).personal(2)), personal);
  const unmeasured = battle();
  unmeasured.characters[1].link = null;
  assert.equal(scoreboardPort({ battle: unmeasured, network: true }).personal(1).at(-1).value, "", "not known yet: the label alone");
});
