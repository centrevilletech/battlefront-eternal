import assert from "node:assert/strict";
import test from "node:test";
import { ClassicSpawning } from "../src/ai/classic/spawning.js";
import { ClassicRandom } from "../src/ai/random.js";
import { CommandPosts } from "../src/commandposts.js";
import { Battle } from "../src/battle.js";
import { HumanSpawn, RESPAWN_DELAY, choosePost, eligiblePosts, spawnPointAt, switchHumanTeam, applyAuthoritativeHumanTeam } from "../src/human-spawn.js";
import { refreshCurves } from "../src/ai/index.js";
import { ClassicDifficulty } from "../src/ai/classic/difficulty.js";

const DT = 1 / 60;

// A post as the AI sees it (src/ai/classic/goals.js postAI).
function post(name, team, { x = 0, spawn = true, ally = false, classPoints = 0, active } = {}) {
  const p = {
    name,
    team,
    position: { x, y: 0, z: 0 },
    ai: {
      spawnPath: spawn ? { name: `${name}Spawn`, present: true, points: [[x, 0, 1, 0], [x + 1, 0, 0, 0.5], [x - 1, 0, 0, 1]] } : null,
      allyPath: ally ? { name: `${name}Ally`, present: true, points: [[x, 0, -1, 0.25]] } : null,
      allyCount: 0,
      classSpawnPoints: classPoints,
      classSpawnWorld: Array.from({ length: classPoints }, (_, i) => [x + 8, 0, i, 0]),
      atk: [100, 10, 10, 10, 10, 10],
      def: [100, 10, 10, 10, 10, 10],
      bans: 0,
      valueBleed: 10,
      yaw: 0,
    },
  };
  if (active !== undefined) p.active = active;
  return p;
}
const relation = (a, b) => (a === 0 || b === 0 ? 0 : a === b ? 1 : -1);
// Theed in a line: the CIS (1) hold the middle, the Republic (2) the ends.
const theed = () => [post("cp2", 2, { x: 0 }), post("cp3", 1, { x: 10 }), post("cp1", 1, { x: 20 }), post("cp6", 2, { x: 30 })];

const ROSTER = { 1: ["cis_inf_battledroid", "cis_inf_droideka"], 2: ["rep_inf_clone_trooper", "rep_inf_clone_sharpshooter", "rep_inf_jet_trooper"] };

// The game's side of HumanSpawn without the Classic AI: a world that places
// him at once (game.js spawnHuman), and records what it was asked.
function ownGate({ auto = null, alive = false } = {}) {
  const calls = [];
  const world = {
    teamNow: 2,
    deadNow: !alive,
    overNow: false,
    classic: null,
    over: () => world.overNow,
    team: () => world.teamNow,
    dead: () => world.deadNow,
    character: () => null,
    roster: (team) => ROSTER[team] || [],
    canSpawn: (className) => className !== "cis_inf_unloaded",
    setTeam(team) {
      calls.push(["team", team]);
      const previous = world.teamNow;
      world.teamNow = team;
      spawn.teamChanged(team, previous);
      return true;
    },
    spawn(className, postName) {
      calls.push(["spawn", className, postName]);
      world.deadNow = false;
      spawn.spawned(className, postName || "cp2");
      return true;
    },
    ready: () => assert.fail("no spawn manager without the Classic AI"),
    kill() {
      calls.push(["kill"]);
      world.deadNow = true;
      spawn.died();
    },
    cancel() {
      calls.push(["cancel"]);
    },
  };
  const spawn = new HumanSpawn({ world, policy: { className: "rep_inf_clone_trooper", team: 2, auto } });
  if (alive) spawn.spawned("rep_inf_clone_trooper", "cp2");
  const events = [];
  for (const name of ["awaitingSpawn", "spawned", "teamChanged", "playerDied"]) spawn.on(name, (detail) => events.push([name, detail]));
  const tick = (commands = null) => {
    spawn.update(DT, commands);
    spawn.flush();
  };
  return { spawn, world, calls, events, tick };
}

test("he starts unspawned, and a spawn choice puts him in the world on the next tick", () => {
  const { spawn, world, calls, events, tick } = ownGate();
  assert.equal(spawn.state().phase, "unspawned");
  tick();
  assert.equal(spawn.state().phase, "unspawned", "nothing happens without a choice");
  tick([{ kind: "spawn", className: "rep_inf_clone_sharpshooter", post: "cp6" }]);
  assert.deepEqual(calls, [["spawn", "rep_inf_clone_sharpshooter", "cp6"]]);
  assert.equal(world.deadNow, false);
  assert.deepEqual(spawn.state(), {
    phase: "alive", team: 2, className: "rep_inf_clone_sharpshooter", classIndex: 1, post: "cp6", canSwitchTeams: true, over: false,
  });
  assert.deepEqual(events.map(([name]) => name), ["spawned"]);
});

test("after a death the screen opens 4 s later, not before", () => {
  const { spawn, events, tick } = ownGate();
  tick([{ kind: "spawn", className: "rep_inf_clone_trooper", post: null }]);
  tick([{ kind: "suicide" }]);
  assert.equal(spawn.state().phase, "dying");
  // 240 ticks of 1/60 make 3.99999...: the screen opens on the 240th.
  for (let i = 1; i < 240; i++) tick();
  assert.equal(spawn.state().phase, "dying");
  tick();
  assert.equal(spawn.state().phase, "awaiting");
  assert.equal(spawn.deadTime >= RESPAWN_DELAY - 1e-6, true);
  assert.deepEqual(events.map(([name]) => name), ["spawned", "playerDied", "awaitingSpawn"]);
  assert.equal(events.at(-1)[1].phase, "awaiting", "the event carries the state");
});

test("a choice made while he is dying waits for the screen's timer", () => {
  const { spawn, calls, tick } = ownGate();
  tick([{ kind: "spawn", className: "rep_inf_clone_trooper", post: null }]);
  tick([{ kind: "suicide" }]);
  tick([{ kind: "spawn", className: "rep_inf_jet_trooper", post: "cp2" }]);
  for (let i = 0; i < 200; i++) tick();
  assert.equal(calls.filter(([kind]) => kind === "spawn").length, 1, "not yet");
  for (let i = 0; i < 40; i++) tick();
  assert.deepEqual(calls.at(-1), ["spawn", "rep_inf_jet_trooper", "cp2"]);
  assert.equal(spawn.state().phase, "alive");
  assert.equal(spawn.state().className, "rep_inf_jet_trooper");
});

test("team switches apply only while he has no unit; suicide only while he has one", () => {
  const { spawn, world, calls, events, tick } = ownGate();
  tick([{ kind: "suicide" }]);
  assert.deepEqual(calls, [], "no unit to kill");
  tick([{ kind: "team", team: 1 }, { kind: "spawn", className: "cis_inf_droideka", post: "cp3" }]);
  assert.deepEqual(calls, [["team", 1], ["spawn", "cis_inf_droideka", "cp3"]]);
  assert.equal(spawn.state().team, 1);
  assert.equal(spawn.state().classIndex, 1);
  tick([{ kind: "team", team: 2 }]);
  assert.equal(world.teamNow, 1, "alive: the switch is refused");
  tick([{ kind: "team", team: 1 }]);
  assert.equal(calls.filter(([kind]) => kind === "team").length, 1, "the same team is no switch");
  assert.ok(events.some(([name, detail]) => name === "teamChanged" && detail.team === 1 && detail.previous === 2));
});

test("a class the team doesn't field, or that can't be built, is refused", () => {
  const { spawn, calls, tick } = ownGate();
  const warn = console.warn;
  const warnings = [];
  console.warn = (...args) => warnings.push(args.join(" "));
  try {
    tick([{ kind: "spawn", className: "cis_inf_droideka", post: null }]);
    assert.deepEqual(calls, []);
    assert.equal(spawn.pending, null);
    assert.equal(spawn.state().phase, "unspawned");
    assert.equal(warnings.length, 1);
  } finally {
    console.warn = warn;
  }
});

test("malformed commands are dropped; a team switch clamps the class into the new roster", () => {
  const { spawn, calls, tick } = ownGate();
  tick([{ kind: "spawn" }, { kind: "launch" }, null, { kind: "team", team: 9 }]);
  assert.deepEqual(calls, []);
  spawn.className = "rep_inf_jet_trooper"; // slot 2 of the Republic's 3
  tick([{ kind: "team", team: 1 }]);
  assert.equal(spawn.state().className, "cis_inf_droideka", "slot 2 clamped to the CIS's last");
});

test("without a screen (the URL named his unit) only the pause menu's RESPAWN applies", () => {
  const { spawn, calls, events, tick } = ownGate({ auto: { team: 2, className: "rep_inf_clone_trooper", post: null }, alive: true });
  assert.equal(spawn.screen, false);
  tick([{ kind: "team", team: 1 }, { kind: "spawn", className: "rep_inf_jet_trooper", post: null }, { kind: "cancel" }]);
  assert.deepEqual(calls, [], "the game confirms the URL's choice itself");
  assert.equal(spawn.pending, null);
  tick([{ kind: "suicide" }]);
  assert.deepEqual(calls, [["kill"]], "RESPAWN is a normal death");
  assert.equal(spawn.state().phase, "dying");
  tick([{ kind: "team", team: 1 }, { kind: "spawn", className: "rep_inf_jet_trooper", post: null }]);
  assert.deepEqual(calls, [["kill"]], "dead, choices are still the game's");
  assert.equal(spawn.pending, null);
  assert.deepEqual(events.map(([name]) => name), ["spawned", "playerDied"], "placed at load, then RESPAWN");
});

test("once the battle is over nobody spawns, and the screen doesn't open again", () => {
  const { spawn, world, calls, events, tick } = ownGate();
  tick([{ kind: "spawn", className: "rep_inf_clone_trooper", post: null }]);
  tick([{ kind: "suicide" }]);
  // A choice made while he was dying, then the battle ends.
  tick([{ kind: "spawn", className: "rep_inf_jet_trooper", post: "cp2" }]);
  world.overNow = true;
  for (let i = 0; i < 300; i++) tick();
  tick([{ kind: "team", team: 1 }, { kind: "spawn", className: "rep_inf_clone_sharpshooter", post: null }]);
  for (let i = 0; i < 10; i++) tick();
  assert.equal(calls.filter(([kind]) => kind === "spawn").length, 1, "only the life before the end");
  assert.equal(calls.some(([kind]) => kind === "team"), false);
  assert.equal(spawn.pending, null);
  assert.equal(spawn.state().phase, "dying");
  assert.equal(spawn.state().over, true);
  assert.equal(events.some(([name]) => name === "awaitingSpawn"), false);
  assert.equal(calls.filter(([kind]) => kind === "cancel").length, 1, "a choice the manager holds is let go, once");
  // Still unspawned at the end: nothing either.
  const fresh = ownGate();
  fresh.world.overNow = true;
  fresh.tick([{ kind: "spawn", className: "rep_inf_clone_trooper", post: null }]);
  assert.deepEqual(fresh.calls, [["cancel"]], "no spawn; only the release at the end");
  assert.equal(fresh.spawn.state().phase, "unspawned");
});

test("cancel withdraws a choice that waits", () => {
  const { spawn, calls, tick } = ownGate();
  tick([{ kind: "spawn", className: "rep_inf_clone_trooper", post: null }]);
  tick([{ kind: "suicide" }]);
  tick([{ kind: "spawn", className: "rep_inf_jet_trooper", post: "cp2" }]);
  assert.deepEqual(spawn.pending, { className: "rep_inf_jet_trooper", post: "cp2" });
  tick([{ kind: "cancel" }]);
  assert.equal(spawn.pending, null);
  assert.deepEqual(calls.at(-1), ["cancel"]);
  for (let i = 0; i < 300; i++) tick();
  assert.equal(spawn.state().phase, "awaiting", "back on the screen, nothing spawned");
  assert.equal(calls.filter(([kind]) => kind === "spawn").length, 1);
});

test("a failing listener doesn't stop the battle", () => {
  const { spawn, tick } = ownGate();
  const error = console.error;
  const errors = [];
  console.error = (...args) => errors.push(args);
  let heard = 0;
  try {
    spawn.on("spawned", () => {
      throw new Error("screen bug");
    });
    const off = spawn.on("spawned", () => heard++);
    tick([{ kind: "spawn", className: "rep_inf_clone_trooper", post: null }]);
    assert.equal(heard, 1);
    assert.equal(errors.length, 1);
    off();
    assert.equal(typeof spawn.on("nonsense", () => {}), "function");
  } finally {
    console.error = error;
  }
});

// The Classic AI's spawn manager runs it: the open rule is its timer, the
// choice goes through onHumanReady, and he spawns on its next update.
const MISSION = [
  { name: "SetTeamName", args: [1, "CIS"] },
  { name: "AddUnitClass", args: [1, "cis_inf_battledroid", 2] },
  { name: "AddUnitClass", args: [1, "cis_inf_droideka", 1] },
  { name: "SetTeamName", args: [2, "Republic"] },
  { name: "AddUnitClass", args: [2, "rep_inf_clone_trooper", 2] },
  { name: "AddUnitClass", args: [2, "rep_inf_clone_sharpshooter", 1] },
  { name: "AddUnitClass", args: [2, "rep_inf_jet_trooper", 1] },
  { name: "SetUnitCount", args: [1, 3] },
  { name: "SetUnitCount", args: [2, 3] },
  { name: "SetReinforcementCount", args: [1, 100] },
  { name: "SetReinforcementCount", args: [2, 100] },
  { name: "SetSpawnDelay", args: [10, 0.25] },
];

function classicGate() {
  const posts = theed();
  const spawned = [];
  let human = null;
  const sp = new ClassicSpawning({
    calls: MISSION,
    posts,
    random: new ClassicRandom(7),
    humans: [{ team: 2 }],
    objectives: null,
    spawnUnit(character, className, at, point) {
      const unit = { alive: true, combat: { health: 1 } };
      if (character.human) {
        human.deadNow = false;
        spawned.push([className, at.name]);
        spawn.spawned(className, at.name);
      }
      return unit;
    },
  });
  const character = sp.human;
  human = {
    deadNow: true,
    classic: { awaitingHuman: (c) => sp.awaitingHuman(c) },
    team: () => character.team,
    dead: () => human.deadNow,
    character: () => character,
    roster: (team) => ROSTER[team] || [],
    canSpawn: () => true,
    setTeam(team) {
      const previous = character.team;
      if (!sp.switchTeam(character, team)) return false;
      spawn.teamChanged(team, previous);
      return true;
    },
    spawn: () => assert.fail("the spawn manager places him"),
    // game.js readyHuman: a choice taken while he waits for a post gives way.
    ready(className, postName) {
      if (!character?.spawn || character.unit) return false;
      character.spawn.ready = false;
      const at = postName ? posts.find((p) => p.name === postName) : undefined;
      if (sp.awaitingHuman(character)) sp.onHumanReady(character, { className, post: at });
      return character.spawn.ready;
    },
    kill() {
      human.deadNow = true;
      sp.onDeath(character);
      spawn.died();
    },
    // game.js cancelHuman: the manager lets go of a choice he waits on.
    cancel() {
      if (character.spawn && !character.unit) character.spawn.ready = false;
    },
    over: () => false,
  };
  const spawn = new HumanSpawn({ world: human, policy: { className: "rep_inf_clone_trooper", team: 2, auto: null } });
  // The game's order in a tick: the manager's clock and Characters, then
  // the screen's choices where it makes its own.
  const tick = (commands = null) => {
    sp.update(DT);
    spawn.update(DT, commands);
    spawn.flush();
  };
  return { sp, character, spawn, spawned, tick, posts };
}

test("Classic: the manager's open rule, its onHumanReady, and its spawn", () => {
  const { sp, character, spawn, spawned, tick } = classicGate();
  assert.equal(sp.awaitingHuman(character), true, "the Character starts with no unit and a spent timer");
  tick();
  assert.equal(spawn.state().phase, "unspawned");
  tick([{ kind: "spawn", className: "rep_inf_clone_sharpshooter", post: "cp6" }]);
  assert.equal(character.spawn.ready, true, "handed to the manager");
  assert.equal(character.className, "rep_inf_clone_sharpshooter");
  assert.equal(spawn.pending, null);
  assert.deepEqual(spawned, [], "placed on the manager's next update");
  tick();
  assert.deepEqual(spawned, [["rep_inf_clone_sharpshooter", "cp6"]]);
  assert.equal(spawn.state().phase, "alive");
});

test("Classic: death, the manager's 4 s timer, a team switch and a new class", () => {
  const { sp, character, spawn, spawned, tick } = classicGate();
  tick([{ kind: "spawn", className: "rep_inf_clone_trooper", post: null }]);
  tick();
  assert.equal(spawn.state().phase, "alive");
  const tickets = sp.tickets(2);
  tick([{ kind: "suicide" }]);
  assert.equal(spawn.state().phase, "dying");
  assert.equal(sp.tickets(2), tickets - 1, "the death spends a ticket");
  let ticks = 0;
  while (spawn.state().phase !== "awaiting") {
    tick();
    assert.ok(++ticks <= 241, "open after 4 s");
  }
  // The manager counts its timer in float32: 4 s run out on the 241st tick.
  assert.equal(ticks, 241);
  tick([{ kind: "team", team: 1 }, { kind: "spawn", className: "cis_inf_droideka", post: "cp3" }]);
  assert.equal(character.team, 1);
  assert.ok(sp.team(1).includes(character) && !sp.team(2).includes(character), "moved between the rosters");
  assert.equal(sp.tickets(2), tickets - 1, "the switch costs nothing");
  tick();
  assert.deepEqual(spawned.at(-1), ["cis_inf_droideka", "cp3"]);
  assert.equal(spawn.state().team, 1);
  assert.equal(spawn.state().phase, "alive");
});

test("Classic: a team switch gives back his place in the old post's queue (no phantom place)", () => {
  const { sp, character } = classicGate();
  const queues = () => Object.fromEntries([...sp.queues].map(([post, queue]) => [post.name, [...queue]]));
  const old = character.spawn.post;
  assert.ok(old && character.spawn.ordinal >= 0, "his place, taken when he was prepared");
  const before = queues();
  assert.equal(before[old.name][3], 1, "a human waiting at his own post");
  assert.equal(sp.switchTeam(character, 1), true);
  const after = queues();
  assert.deepEqual(after[old.name], [before[old.name][0], before[old.name][1], before[old.name][2], 0], "given back as his own post's");
  const next = character.spawn.post;
  assert.equal(next.team, 1);
  assert.equal(after[next.name][3], (before[next.name]?.[3] ?? 0) + 1, "taken at the new team's post");
  // Back and forth: always exactly his one place, nothing below zero.
  for (const team of [2, 1, 2]) assert.equal(sp.switchTeam(character, team), true);
  const queued = Object.values(queues());
  for (const queue of queued) assert.ok(queue.every((n) => n >= 0), JSON.stringify(queues()));
  assert.equal(queued.reduce((sum, queue) => sum + queue[1] + queue[3], 0), 1);
  // A bot queueing at his old post counts only a human really waiting there.
  const queue = queues()[old.name];
  const humanHere = character.spawn.post === old ? 1 : 0;
  assert.equal(queue[3], humanHere);
  assert.equal(sp.reserve(old, false, true), queue[2] + humanHere);
});

test("Classic: cancel takes back a choice the manager holds while he waits for a post", () => {
  const { sp, character, spawn, spawned, tick, posts } = classicGate();
  // Every post is the CIS's: the Republic has nowhere to spawn.
  const owners = posts.map((post) => post.team);
  for (const post of posts) post.team = 1;
  tick([{ kind: "spawn", className: "rep_inf_jet_trooper", post: null }]);
  assert.equal(character.spawn.ready, true, "the manager took it");
  for (let i = 0; i < 30; i++) tick();
  assert.deepEqual(spawned, [], "no post: he waits, ready");
  assert.equal(spawn.state().phase, "unspawned");
  tick([{ kind: "cancel" }]);
  assert.equal(character.spawn.ready, false, "let go, as the original's BACK");
  assert.equal(spawn.pending, null);
  // A post comes back: without a choice he doesn't spawn.
  posts.forEach((post, i) => { post.team = owners[i]; });
  for (let i = 0; i < 30; i++) tick();
  assert.deepEqual(spawned, []);
  // A new choice is taken again.
  tick([{ kind: "spawn", className: "rep_inf_clone_trooper", post: "cp2" }]);
  tick();
  assert.deepEqual(spawned, [["rep_inf_clone_trooper", "cp2"]]);
  assert.equal(sp.awaitingHuman(character), false);
});

test("a team switch gives the living bots the aim curves of their side as it now stands", () => {
  const difficulty = new ClassicDifficulty({ tier: 2, playerTeam: 2 });
  assert.notEqual(difficulty.curves(1), difficulty.curves(2), "player and enemy curves differ");
  const ai = { curvesFor: (team) => difficulty.curves(team) };
  const cis = { unit: { team: 1 }, difficulty: difficulty.curves(1) };
  const rep = { unit: { team: 2 }, difficulty: difficulty.curves(2) };
  const idle = { unit: null, difficulty: "kept" };
  difficulty.playerTeam = 1;
  refreshCurves(ai, [cis, rep, idle]);
  assert.equal(cis.difficulty, difficulty.player, "his new side's bots take the player curves");
  assert.equal(rep.difficulty, difficulty.enemy);
  assert.equal(idle.difficulty, "kept", "a controller without a unit takes its curves at its spawn");
});

test("switchHumanTeam moves every place the team is kept, or none", () => {
  const audio = { fields: null, retarget(fields) { this.fields = fields; } };
  const holders = (ai) => {
    const h = {
      teams: null,
      setTeams: (own, foe) => { h.teams = [own, foe]; },
      character: { team: 2 },
      ai,
      posts: new CommandPosts(null, { commandPosts: [], mission: { teams: { 1: { name: "CIS" }, 2: { name: "Republic" } } } }, { logos: new Map(), textures: {}, playerTeam: 2, headless: true }),
      battle: new Battle({ teams: { 1: {}, 2: {} }, posts: { posts: [] }, own: 2, foe: 1 }),
      live: { playerTeam: 2 },
      combats: [{ team: 2 }, { team: 2 }],
      unit: { team: 2 },
      audio,
    };
    return h;
  };
  // Without the Classic AI: the Character moves itself.
  const h = holders(null);
  assert.equal(switchHumanTeam(h, 1), true);
  assert.deepEqual(h.teams, [1, 2]);
  assert.equal(h.character.team, 1);
  assert.equal(h.posts.playerTeam, 1);
  assert.equal(h.posts.relation(1), "friendly");
  assert.equal(h.posts.relation(2), "enemy");
  assert.deepEqual([h.battle.own, h.battle.foe], [1, 2]);
  assert.equal(h.live.playerTeam, 1);
  assert.deepEqual(h.combats.map((c) => c.team), [1, 1]);
  assert.equal(h.unit.team, 1);
  assert.deepEqual(audio.fields, { team: 1, foe: 2 });
  // With it: its roster decides; refused, nothing changes.
  const refusing = holders({ switchTeam: () => false });
  assert.equal(switchHumanTeam(refusing, 1), false);
  assert.equal(refusing.teams, null);
  assert.equal(refusing.posts.playerTeam, 2);
  assert.equal(refusing.unit.team, 2);
  const moved = [];
  const accepting = holders({ switchTeam: (character, team) => moved.push(team) > 0 });
  assert.equal(switchHumanTeam(accepting, 1), true);
  assert.deepEqual(moved, [1]);
  assert.equal(accepting.character.team, 2, "the AI moves the Character itself");
  // Only teams 1 and 2.
  assert.equal(switchHumanTeam(holders(null), 3), false);
});

test("eligible posts, the screen's post choice, and spawn points", () => {
  const posts = [...theed(), post("neutral", 0, { x: 40 }), post("bare", 2, { x: 50, spawn: false }), post("off", 2, { x: 60, active: false })];
  assert.deepEqual(eligiblePosts(posts, 2, relation).map((p) => p.name), ["cp2", "cp6"]);
  assert.deepEqual(eligiblePosts(posts, 1, relation).map((p) => p.name), ["cp3", "cp1"]);
  // An eligible previous choice is kept.
  assert.equal(choosePost(posts, 2, posts[3], relation).name, "cp6");
  // An ineligible one moves to the eligible post nearest it.
  assert.equal(choosePost(posts, 2, posts[2], relation).name, "cp6", "cp1 (x 20) is nearest cp6 (x 30)");
  // No previous choice: the eligible post nearest an enemy-held post.
  assert.equal(choosePost(posts, 2, null, relation).name, "cp2", "cp2 (x 0) is 10 from cp3");
  // Nothing held by an enemy: a random eligible post.
  const quiet = [post("a", 2, { x: 0 }), post("b", 2, { x: 5 }), post("c", 0, { x: 9 })];
  assert.equal(choosePost(quiet, 2, null, relation, () => 0.99).name, "b");
  assert.equal(choosePost(quiet, 2, null, relation, () => 0).name, "a");
  assert.equal(choosePost([post("x", 1)], 2, null, relation), null);
  // Spawn points: the spawn path, an ally's path, class points, the post.
  assert.deepEqual(spawnPointAt(posts[0], 2, () => 0.5), [1, 0, 0, 0.5]);
  const allied = post("ally", 1, { x: 70, ally: true });
  assert.deepEqual(spawnPointAt(allied, 3, () => 0), [70, 0, -1, 0.25]);
  assert.deepEqual(spawnPointAt(post("cls", 2, { x: 80, spawn: false, classPoints: 2 }), 2, () => 0.6), [88, 0, 1, 0]);
  assert.deepEqual(spawnPointAt(post("none", 2, { x: 90, spawn: false }), 2, () => 0), [90, 0, 0, 0]);
});


test('authoritative human team presentation bypasses request policy without running spawn choices',()=>{
 const calls=[],h={character:{team:2},unit:{team:2},live:{playerTeam:2},combats:[{team:2}],
  setTeams:(...v)=>calls.push(['teams',...v]),posts:{setPlayerTeam:t=>calls.push(['posts',t])},
  battle:{setHumanTeam:(...v)=>calls.push(['battle',...v])},audio:{retarget:v=>calls.push(['audio',v])},
  ai:{switchTeam(){throw Error('must not run spawn policy');},presentHumanTeam:t=>calls.push(['ai-presentation',t])}};
 assert.equal(applyAuthoritativeHumanTeam(h,1),true);
 assert.deepEqual([h.character.team,h.unit.team,h.live.playerTeam,h.combats[0].team],[1,1,1,1]);
 assert.deepEqual(calls,[['teams',1,2],['posts',1],['battle',1,2],['audio',{team:1,foe:2}],['ai-presentation',1]]);
});

test("an authority's account of the player raises the same events as the local flow", () => {
  const world = { team: () => 2, dead: () => true, character: () => null, classic: null, roster: () => ["cis_inf_battledroid"], canSpawn: () => true, over: () => false };
  const spawn = new HumanSpawn({ world, policy: { className: "cis_inf_battledroid", auto: null } });
  const events = [];
  for (const name of ["awaitingSpawn", "spawned", "teamChanged", "playerDied"]) spawn.on(name, (detail) => events.push([name, detail]));
  const account = (phase, extra = {}) => ({ phase, className: "cis_inf_battledroid", post: null, spawnedOnce: phase !== "unspawned", ended: false, deadTime: 0, pending: null, auto: null, ...extra });
  spawn.applyAuthoritative(account("unspawned"));
  assert.equal(spawn.phase, "unspawned"); assert.deepEqual(events, [], "the screen is already open at the start");
  spawn.applyAuthoritative(account("alive", { post: "cp1" }));
  assert.deepEqual(events.at(-1), ["spawned", { team: 2, className: "cis_inf_battledroid", post: "cp1" }]);
  spawn.applyAuthoritative(account("dying", { deadTime: 1 }));
  assert.deepEqual(events.at(-1), ["playerDied", { team: 2, className: "cis_inf_battledroid" }]);
  spawn.applyAuthoritative(account("awaiting", { deadTime: 4 }));
  assert.equal(events.at(-1)[0], "awaitingSpawn"); assert.equal(events.at(-1)[1].phase, "awaiting");
  spawn.applyAuthoritative(account("awaiting", { deadTime: 5 }));
  assert.equal(events.length, 3, "repeated accounts raise nothing");
  spawn.applyAuthoritative(account("alive"));
  assert.equal(events.at(-1)[0], "spawned"); assert.equal(spawn.spawnedOnce, true);
  spawn.canSwitchTeams = false;
  assert.equal(spawn.state().canSwitchTeams, false, "a server that assigns teams hides the switch");
});
