import test from "node:test";
import assert from "node:assert/strict";
import { SpawnModel, CHOOSING, COMMITTED, CLOSED } from "../src/spawn-screen/model.js";
import { FakeSpawnPort, geonosisData } from "./spawn-screen-fake-port.js";

function setup(options = {}) {
  const port = new FakeSpawnPort({ ...geonosisData(), ...options });
  const model = new SpawnModel({ port, random: options.random || (() => 0) });
  const events = [];
  for (const name of ["open", "close", "class", "team", "post", "commit"]) model.on(name, (detail) => events.push([name, detail]));
  return { port, model, events };
}

test("opens with the player's team and class 0 at the start, and the front-line post", () => {
  const { model, events } = setup();
  model.open("start");
  assert.equal(model.state, CHOOSING);
  assert.equal(model.team, 1);
  assert.equal(model.classIndex, 0);
  assert.equal(model.className, "rep_inf_clone_trooper");
  // cp10 and cp2 are the Republic's; cp10 is nearer the CIS posts.
  assert.equal(model.post, "cp10");
  assert.deepEqual(events.map(([name]) => name), ["open", "post"]);
  assert.equal(events[0][1].first, true);
});

test("after a death the class is the one last played and the post is remembered", () => {
  const { model, port } = setup({ human: { phase: "awaiting", team: 1, classIndex: 3 } });
  model.open("death");
  assert.equal(model.classIndex, 3);
  model.cyclePost();
  model.cyclePost();
  const picked = model.post;
  model.close();
  port.human.classIndex = 2;
  model.open("death");
  assert.equal(model.classIndex, 2);
  assert.equal(model.post, picked);
});

test("up wraps to the last class, down wraps to the first", () => {
  const { model } = setup();
  model.open();
  assert.ok(model.previousClass());
  assert.equal(model.classIndex, 4);
  assert.ok(model.nextClass());
  assert.equal(model.classIndex, 0);
  assert.ok(model.selectClass(2));
  assert.equal(model.selectClass(2), false, "picking the picked class changes nothing");
  assert.equal(model.selectClass(9), false);
});

test("a team switch resets the class to 0 and picks a random eligible post", () => {
  const random = () => 0.99;
  const { model, events } = setup({ random });
  model.open();
  model.selectClass(3);
  events.length = 0;
  assert.ok(model.switchTeam("mouse"));
  assert.equal(model.team, 2);
  assert.equal(model.classIndex, 0);
  assert.equal(model.className, "cis_inf_battledroid");
  const cis = ["cp6", "cp14", "cp13", "cp7", "cp3"];
  assert.equal(model.post, cis.at(-1), "random 0.99 picks the last eligible post");
  assert.deepEqual(events.filter(([name]) => name === "team").map(([, d]) => d), [{ team: 2, source: "mouse" }]);
});

test("no team switch where the mission forbids it", () => {
  const { model } = setup({ canSwitchTeams: false });
  model.open();
  assert.equal(model.switchTeam(), false);
  assert.equal(model.team, 1);
});

test("a server's welcome that assigns teams takes the switch away from a screen already open", () => {
  const { model, port } = setup();
  model.open("start");
  assert.equal(model.canSwitchTeams, true, "offline, or before the welcome, he may pick");
  port.human.canSwitchTeams = false;
  assert.equal(model.canSwitchTeams, false, "the server assigns teams: no switch, with no reopening");
  assert.equal(model.switchTeam("mouse"), false);
  assert.equal(model.team, 1);
});

test("Tab cycles the team's eligible posts with the team's own counter", () => {
  const { model } = setup();
  model.open();
  const seen = [];
  for (let i = 0; i < 3; i++) {
    model.cyclePost();
    seen.push(model.post);
  }
  assert.deepEqual(seen, ["cp10", "cp2", "cp10"]);
});

test("a click takes only a post the picked team itself holds", () => {
  const { model } = setup();
  model.open();
  assert.equal(model.selectPost("cp6"), false, "an enemy post");
  assert.ok(model.selectPost("cp2"));
  assert.equal(model.post, "cp2");
});

test("a lost post moves the pick to the nearest eligible one", () => {
  const { model, port, events } = setup();
  model.open();
  model.selectPost("cp2");
  events.length = 0;
  port.data.posts.find((post) => post.name === "cp2").owner = 2;
  model.check();
  assert.equal(model.post, "cp10");
  assert.deepEqual(events, [["post", { post: "cp10", reason: "check" }]]);
});

test("SPAWN commits the choice once, and the world's spawn closes it", () => {
  const { model, port } = setup();
  model.open();
  model.selectClass(1);
  const request = model.commit();
  assert.deepEqual(request, { team: 1, className: "rep_inf_arc_trooper", post: "cp10" });
  assert.equal(model.state, COMMITTED);
  assert.equal(model.commit(), null);
  assert.equal(model.selectClass(2), false, "no changes once committed");
  model.uncommit();
  assert.equal(model.state, CHOOSING);
  model.close();
  assert.equal(model.state, CLOSED);
  port.request(request);
  port.step();
  assert.equal(port.human.phase, "alive");
});

test("no post, no spawn", () => {
  const { model, port } = setup();
  for (const post of port.data.posts) post.owner = 2;
  model.open();
  assert.equal(model.post, null);
  assert.equal(model.commit(), null);
});

test("the open screen follows a team the world assigns, even where the player may not switch", () => {
  // Online: the screen opens on the URL's side before the server's team
  // assignment arrives; spawning must then ask for the assigned team.
  const { model, events } = setup({ human: { phase: "awaiting", team: 1, classIndex: 2, canSwitchTeams: false } });
  model.open("start");
  assert.equal(model.team, 1);
  events.length = 0;
  assert.ok(model.followTeam(2));
  assert.equal(model.team, 2);
  assert.equal(model.classIndex, 0);
  assert.ok(model.className.startsWith("cis_"), model.className);
  assert.deepEqual(events.map(([name]) => name).slice(0, 3), ["team", "class", "post"]);
  assert.equal(events[0][1].source, "world");
  assert.equal(model.followTeam(2), false, "already there");
  model.close();
  assert.equal(model.followTeam(1), false, "a closed screen reads the team when it opens");
});
