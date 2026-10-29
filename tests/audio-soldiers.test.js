import test from "node:test";
import assert from "node:assert/strict";
import { Object3D } from "three";
import { AudioEngine } from "../src/audio/engine.js";
import { listenSounds } from "../src/audio/events.js";
import { Chatter } from "../src/audio/chatter.js";
import { hashKey } from "../src/audio/hash.js";
import { PlayerSounds } from "../src/audio/player.js";
import { SoldierSounds } from "../src/audio/soldiers.js";
import { Streams } from "../src/audio/streams.js";
import { Soldier } from "../src/soldier.js";
import { SoldierPresentation } from "../src/soldier-presentation.js";
import { PlayerCombat } from "../src/combat.js";
import { Arsenal } from "../src/weapons.js";
import { Unit } from "../src/units.js";
import { CollisionWorld } from "../src/collision.js";
import { EntityGameState } from "../src/runtime/network-entity-game.js";
import { captureOwnPlayer } from "../src/runtime/network-entity-player.js";
import { createSnapshotSender } from "./snapshot-send.js";

// Other soldiers' weapons and voices (src/audio/soldiers.js), their chatter
// (src/audio/chatter.js, src/audio/streams.js), and the engine's out of
// earshot rule, with made-up sound data (no retail assets).

const CLASSES = {
  rifle: { FireSound: "shot", ReloadSound: "reload", OverheatSound: "hot", OverheatSoundPitch: "0.5" },
  burst: { FireSound: "crack", FireSoundStop: "1" },
  cutter: { FireLoopSound: "cutting" },
  trooper: { HurtSound: "ouch", DeathSound: "argh", FleeSound: "grenade", [hashKey("AISCFieldMoveOutSound").slice(2)]: "ai_moveout", [hashKey("SCFieldMoveOutSound").slice(2)]: "moveout" },
};

function rig({ units = [], own = null } = {}) {
  const played = [];
  const audio = {
    game: { combat: own, units: { all: () => units } },
    engine: { time: 0 },
    data: { classSounds: (cls) => CLASSES[cls] || null, sound: () => null },
    playClass: (cls, key, options) => {
      const voice = { cls, key, options, stopped: false, pitch: null, stop() { this.stopped = true; }, setAbsolutePitch(p) { this.pitch = p; } };
      played.push(voice);
      return voice;
    },
  };
  return { audio, played, soldiers: new SoldierSounds(audio) };
}

function bot(className = "trooper", weapons = [{ def: { class: "rifle" } }], at = { x: 5, y: 0, z: 7 }) {
  const combat = { owner: null, soldier: { position: at }, health: 100, dead: false, arsenal: { weapons } };
  const unit = { className, removed: false, combat };
  combat.owner = unit;
  return unit;
}

test("another soldier's shot plays his weapon's FireSound at him, once a frame, as the player's own", () => {
  const unit = bot();
  const { audio, played, soldiers } = rig({ units: [unit] });
  const weapon = unit.combat.arsenal.weapons[0];
  soldiers.fire(unit.combat, { weapon });
  soldiers.fire(unit.combat, { weapon });
  assert.equal(played.length, 1, "a salvo's rounds in one frame sound once");
  assert.equal(played[0].key, "FireSound");
  assert.equal(played[0].cls, "rifle");
  assert.equal(played[0].options.follow(), unit.combat.soldier.position, "at his feet, following him");
  audio.engine.time += 1 / 60;
  soldiers.fire(unit.combat, { weapon });
  assert.equal(played.length, 2);
  assert.equal(played[0].stopped, false, "a FireSound lets the one before it finish");
  // The player's own weapon plays the same, through the same rules.
  const player = new PlayerSounds(audio);
  const position = { x: 0, y: 0, z: 0 };
  audio.game.soldier = { position };
  player.fire(null, { weapon: { def: { class: "rifle" } } });
  assert.equal(played.at(-1).key, "FireSound");
  assert.equal(played.at(-1).options.follow(), position);
});

test("FireSoundStop cuts the shot before; the fusion cutter's loop stops when he stops", () => {
  const burst = { def: { class: "burst" } };
  const cutter = { def: { class: "cutter" }, firing: true };
  const unit = bot("trooper", [burst, cutter]);
  const { audio, played, soldiers } = rig({ units: [unit] });
  soldiers.fire(unit.combat, { weapon: burst });
  audio.engine.time += 1 / 60;
  soldiers.fire(unit.combat, { weapon: burst });
  assert.equal(played[0].stopped, true);
  soldiers.fire(unit.combat, { weapon: cutter, repair: true });
  const loop = played.at(-1);
  assert.equal(loop.key, "FireLoopSound");
  assert.equal(loop.options.loop, true);
  soldiers.update(1 / 60);
  assert.equal(loop.stopped, false, "on while he works");
  cutter.firing = false;
  soldiers.update(1 / 60);
  assert.equal(loop.stopped, true);
});

test("his reload and overheat play at him; his loops end with his life; the player's are not his", () => {
  const rifle = { def: { class: "rifle", HeatThreshold: 1 }, reloading: false, overheated: false, heat: 0 };
  const unit = bot("trooper", [rifle]);
  const ownRifle = { def: { class: "rifle" }, reloading: true };
  const own = { soldier: { position: {} }, arsenal: { weapons: [ownRifle] } };
  const { played, soldiers } = rig({ units: [unit, { className: "trooper", removed: false, combat: own }], own });
  soldiers.update(1 / 60);
  assert.equal(played.length, 0, "nothing until something changes; the player's own are PlayerSounds'");
  rifle.reloading = true;
  soldiers.update(1 / 60);
  assert.equal(played[0].key, "ReloadSound");
  rifle.reloading = false;
  soldiers.update(1 / 60);
  assert.equal(played[0].stopped, true, "cut off when the reload is done");
  rifle.overheated = true;
  rifle.heat = 2;
  soldiers.update(1 / 60);
  const hot = played.at(-1);
  assert.equal(hot.key, "OverheatSound");
  assert.equal(hot.options.loop, true);
  assert.equal(hot.pitch, 0.75, "half way cooled: lerp(0.5, 1, threshold / heat)");
  unit.combat.dead = true;
  soldiers.update(1 / 60);
  assert.equal(hot.stopped, true, "a dead soldier's loop stops");
});

test("his voice: hurt by a hit over 2% of the health it leaves, killed always, one voice each cuts off", () => {
  const unit = bot();
  const { played, soldiers } = rig({ units: [unit] });
  const combat = unit.combat;
  combat.health = 100;
  soldiers.damage(combat, 2, false);
  assert.equal(played.length, 0, "2 of the 100 left is not over 2%");
  soldiers.damage(combat, 2.5, false);
  assert.equal(played.length, 1);
  assert.equal(played[0].cls, "trooper");
  assert.equal(played[0].key, "HurtSound");
  assert.equal(played[0].options.category, "voice");
  assert.equal(played[0].options.follow(), combat.soldier.position);
  combat.health = 0;
  soldiers.damage(combat, 0, true);
  assert.equal(played[0].stopped, true, "the death cuts the hurt off");
  assert.equal(played[1].key, "DeathSound");
  // Another soldier's voice is his own.
  const other = bot();
  soldiers.damage(other.combat, 50, false);
  assert.equal(played[1].stopped, false);
});

// --- the engine ------------------------------------------------------------

function engineRig(sounds) {
  const engine = new AudioEngine();
  engine.ctx = { state: "running", createBuffer: (channels, frames, rate) => ({ duration: frames / rate, sampleRate: rate, getChannelData: () => new Float32Array(frames) }) };
  engine.setData({
    sound: (name) => sounds[name] || null,
    samples: { s: [0, 2205, 22050] },
    sample: () => ({ pcm: new Int16Array(2205), rate: 22050, frames: 2205 }),
    buses: () => new Map(),
  });
  engine.setListener({ x: 0, y: 0, z: 0 }, { x: 0, y: 0, z: -1 }, { x: 0, y: 1, z: 0 });
  return engine;
}

test("a one-shot asked for out of earshot takes no voice; loops and 2D sounds still start", () => {
  const shot = { type: "SoundProperties", Name: "shot", Mode3D: 1, MinDistance: 11, MaxDistance: 400, MuteDistance: 400, RollOff: 3.5, SampleList: [["s", 1]] };
  const engine = engineRig({ shot, hum: { ...shot, Name: "hum", Looping: 1 }, flat: { ...shot, Name: "flat", Mode3D: 0 } });
  assert.equal(engine.play("shot", { position: { x: 0, y: 0, z: -500 } }), null);
  assert.equal(engine.stats.culled, 1);
  // The listener is 2 m ahead of the camera: 401 m from the camera is 399 m.
  assert.ok(engine.play("shot", { position: { x: 0, y: 0, z: -401 } }));
  const at = { x: 600, y: 0, z: 0 };
  assert.equal(engine.play("shot", { follow: () => at }), null, "where a moving source is now");
  assert.ok(engine.play("hum", { position: { x: 0, y: 0, z: -500 } }), "a loop may come into earshot");
  assert.ok(engine.play("flat", { position: { x: 0, y: 0, z: -500 } }), "2D has no distance");
  assert.equal(engine.stats.culled, 2);
});

// --- chatter -----------------------------------------------------------------

const LINE = { type: "SoundStreamProperties", Stream: hashKey("cw_vo"), Mode3D: 1, Gain: 0.7, Bias: 0.0001, Priority: 0.6, MinDistance: 16, MaxDistance: 150, MuteDistance: 150, RollOff: 3 };

function streamRig(sounds) {
  const engine = { time: 0, busGain: () => 1, listener: { ear: { x: 0, y: 0, z: 0 } } };
  const streams = new Streams(engine);
  const index = { name: "cw_vo", channels: 1, segments: { "0x1": [0, 100, 1000, 22050], "0x2": [100, 100, 1000, 22050] } };
  streams.setData({ streams: new Map([["cw_vo", index]]), mission: { streams: [["sound\\cw.lvl", "cw_vo"]] }, sound: (name) => sounds[name] || null });
  return { engine, streams, queue: () => streams.queues.get("cw_vo").map((r) => r.position?.z ?? "2D") };
}

test("chatter queues on its stream's slot by score: nearer first, four at most", () => {
  const { streams, queue } = streamRig({ line: { ...LINE, Name: "line", SegmentList: [["0x1", 1]] }, order: { ...LINE, Name: "order", Mode3D: 0, Gain: 0.01, Bias: 0, SegmentList: [["0x2", 1]] } });
  for (const z of [100, 10, 140, 50]) assert.equal(streams.queue("line", { position: { x: 0, y: 0, z } }), true);
  assert.deepEqual(queue(), [10, 50, 100, 140]);
  assert.equal(streams.queue("line", { position: { x: 0, y: 0, z: 145 } }), false, "a fifth said further off than all isn't queued");
  assert.deepEqual(queue(), [10, 50, 100, 140]);
  assert.equal(streams.queue("line", { position: { x: 0, y: 0, z: 30 } }), true);
  assert.deepEqual(queue(), [10, 30, 50, 100], "a nearer fifth drops the furthest");
  assert.equal(streams.queue("order"), true, "a 2D line that would be last drops the line ahead of it instead");
  assert.deepEqual(queue(), [10, 30, 50, "2D"]);
});

test("a line said out of earshot still queues and holds the slot; PlayInterval holds every speaker", () => {
  const { engine, streams, queue } = streamRig({ far: { ...LINE, Name: "far", PlayInterval: 20, CyclePlayback: 1, SegmentList: [["0x1", 1], ["0x2", 1]] } });
  assert.equal(streams.queue("far", { position: { x: 0, y: 0, z: 500 } }), true, "Bias x Priority: never 0");
  assert.ok(streams.queues.get("cw_vo")[0].score > 0 && streams.queues.get("cw_vo")[0].score < 1e-4);
  const state = streams.gateOf(streams.data.sound("far"));
  const cycle = state.cycle;
  engine.time = 5;
  assert.equal(streams.queue("far", { position: { x: 0, y: 0, z: 20 } }), false, "another soldier's within the interval: no line");
  assert.notEqual(state.cycle, cycle, "its segment was picked first, as the original game does");
  engine.time = 21;
  assert.equal(streams.queue("far", { position: { x: 0, y: 0, z: 20 } }), true);
  assert.deepEqual(queue(), [20, 500]);
});

test("a vo record plays the speaker's class line: queued 3D where he said it, or the player's own at once, 2D", () => {
  const queued = [];
  const said = [];
  const sounds = CLASSES.trooper;
  const audio = {
    classSound: (cls, key) => (cls === "trooper" && sounds[key] ? { name: sounds[key] } : null),
    streams: { queue: (name, options) => queued.push({ name, options }), say: (name, options) => said.push({ name, options }) },
  };
  const chatter = new Chatter(audio);
  const unit = { className: "trooper" };
  chatter.say({ kind: "vo", source: { unit }, position: [1, 2, 3], property: "FleeSound" });
  assert.deepEqual(queued[0], { name: "grenade", options: { category: "voice", position: { x: 1, y: 2, z: 3 }, spatial: true } });
  chatter.say({ kind: "vo", source: { unit }, position: [4, 5, 6], property: "AISCFieldMoveOutSound", positional: true });
  assert.equal(queued[1].name, "ai_moveout", "the command bank's keys are hashed");
  // A bot in a seat speaks with his own class's lines.
  chatter.say({ kind: "vo", source: { unit: { className: "rep_hover_fightertank", occupant: unit } }, position: [0, 0, 0], property: "AISCFieldMoveOutSound" });
  assert.equal(queued[2].name, "ai_moveout");
  chatter.say({ kind: "vo", source: { unit }, position: [0, 0, 0], property: "SCFieldMoveOutSound", positional: false });
  assert.deepEqual(said, [{ name: "moveout", options: { category: "voice" } }]);
  chatter.say({ kind: "vo", source: { unit }, position: [0, 0, 0], property: "ChargeSound" });
  assert.equal(queued.length + said.length, 4, "a property his class doesn't set says nothing");
});

// --- online --------------------------------------------------------------------

const registry = { contentEpoch: 1, classes: new Map([[1, "trooper"]]), weapons: new Map([[1, "rifle"]]), inputCodes: [], bindingProfiles: new Map([[0, {}]]) };
function body(id) {
  const world = new CollisionWorld(new Float32Array([-100, 0, -100, 100, 0, -100, 100, 0, 100, -100, 0, 100]), new Uint32Array([0, 2, 1, 0, 3, 2]), new Uint32Array([0, 1]));
  const soldier = new Soldier(world, {});
  soldier.spawn(0, 0, 0, 0);
  const combat = Object.create(PlayerCombat.prototype);
  Object.assign(combat, { soldier, arsenal: new Arsenal([{ class: "rifle", channel: 0, RoundsPerClip: 30, ammo: 3, base: "cannon" }]), health: 100, dead: false, deathTime: 0, zoomStep: 0, firstPerson: true, kick: { rate: 0, hold: 0, offset: 0 }, updateHeld() {} });
  const unit = new Unit({ className: "trooper", def: {}, soldier, combat, presentation: new SoldierPresentation(soldier), actor: { root: new Object3D() }, animator: { reset() {}, die() {} } });
  Object.assign(unit, { id, generation: 1, team: 1, removed: false, alive: true });
  return unit;
}

test("online, another player's replicated health sounds his hurts, and his death once", async () => {
  const { ENTITY_SOLDIER_STATES } = await import("../src/runtime/network-entity-capture.js");
  const own = body(1);
  const remote = body(2);
  const map = new Map([[1, own], [2, remote]]);
  const game = { playerTeam: 1, applyAuthoritativeHumanTeam() {}, ordnance: { clearProjectilePresentation() {} }, playerUnit: own, units: { byId: (id) => map.get(id), byIdMap: map }, vehicles: { vehicles: [] }, client: { seats: {} }, factory: { build() { throw Error("unexpected"); } } };
  const adapter = new EntityGameState(game, registry);
  const heard = [];
  const unlisten = listenSounds((type, combat, amount) => heard.push([type, combat === remote.combat, amount]));
  try {
    const record = (tick, health, state = 0, generation = 1) => ({ kind: "soldier", id: 2, generation, tick, classId: 1, team: 2, position: [20, 0, 0], velocity: [0, 0, 0], yaw: 0, pitch: 0, health, posture: 0, state, weapon: 1, fireCounter: 0 });
    const sender = createSnapshotSender(adapter);
    const packet = (tick, id, records) => sender.send(tick, [captureOwnPlayer({ unit: own, tick, epoch: 1, registry, classId: 1 }), ...records], { ackInput: id });
    adapter.baseline();
    packet(100, 1, [record(100, 100)]);
    packet(101, 2, [record(101, 70)]);
    packet(102, 3, [record(102, 70)]);
    const dead = ENTITY_SOLDIER_STATES.indexOf("dead");
    packet(103, 4, [record(103, 0, dead)]);
    packet(104, 5, [record(104, 100, 0, 2)]);
    assert.deepEqual(heard.filter(([type]) => type.startsWith("actor")), [["actorHurt", true, 30], ["actorDeath", true, 0]], "a drop hurts; the killing record is a death alone; a new life is neither");
  } finally {
    unlisten();
  }
});
