import test from "node:test";
import assert from "node:assert/strict";
import { createSpawnScreen } from "../src/spawn-screen/index.js";
import { listenSounds } from "../src/audio/events.js";
import { FakeSpawnPort, geonosisData } from "./spawn-screen-fake-port.js";

// Just enough of the battle HUD for the screen's logic (no drawing).
function hudStub() {
  return {
    renderer: null, height: 768, strings: {}, battle: { over: false }, postIconSize: 15, layer: null,
    fonts: { lineHeight: () => 18, width: () => 0 },
    setLayer(layer) { this.layer = layer; },
  };
}

function setup(options = {}) {
  const port = new FakeSpawnPort({ ...geonosisData(), ...options });
  const hud = hudStub();
  const controls = { enabled: true, cleared: 0, locks: 0, clear() { this.cleared += 1; }, lock() { this.locks += 1; } };
  const sounds = [];
  const unlisten = listenSounds((type, effect) => sounds.push([type, effect]));
  const screen = createSpawnScreen({ port, hud, controls, clock: "manual", random: () => 0 });
  return { port, hud, controls, sounds, screen, unlisten };
}

test("opens as the map loads: the HUD layer, the controls off, the first camera shot", (t) => {
  const { port, hud, controls, sounds, screen, unlisten } = setup();
  t.after(unlisten);
  assert.ok(screen.isOpen);
  assert.equal(hud.layer, screen.view.layer);
  assert.equal(controls.enabled, false);
  assert.deepEqual(port.viewShots, [port.shots[0]]);
  assert.deepEqual(sounds, [["interface", "SpawnDisplaySpawnPointChange"]], "the post's default pick");
});

test("SPAWN sends the choice; the world's spawn closes the screen", (t) => {
  const { port, hud, controls, sounds, screen, unlisten } = setup();
  t.after(unlisten);
  screen.update(1 / 60);
  assert.deepEqual(screen.spawn(), { team: 1, className: "rep_inf_clone_trooper", post: "cp10" });
  assert.equal(controls.locks, 1, "the mouse is asked for again from the click");
  port.step();
  assert.equal(screen.isOpen, false);
  assert.equal(hud.layer, null);
  assert.equal(controls.enabled, true);
  assert.equal(port.viewShots.at(-1), null);
  assert.deepEqual(sounds.at(-1), ["interface", "SpawnDisplaySpawnPointAccept"]);
});

test("after a death it reopens on a camera shot, with the class last played", (t) => {
  const { port, screen, unlisten } = setup();
  t.after(unlisten);
  screen.model.selectClass(2);
  screen.spawn();
  port.step();
  port.await();
  assert.ok(screen.isOpen);
  assert.equal(screen.model.classIndex, 2);
  assert.equal(port.viewShots.length, 3);
  assert.ok(port.shots.includes(port.viewShots.at(-1)));
});

test("a choice the world doesn't take comes back, and is withdrawn", (t) => {
  const { port, screen, unlisten } = setup({ autoSpawn: false });
  t.after(unlisten);
  screen.spawn();
  for (let i = 0; i < 30; i++) screen.update(1 / 60);
  assert.equal(screen.model.state, "committed");
  for (let i = 0; i < 40; i++) screen.update(1 / 60);
  assert.equal(screen.model.state, "choosing");
  assert.equal(port.cancels, 1);
  assert.equal(port.requests.length, 0);
});

test("the battle's end closes it and takes a sent choice back", (t) => {
  const { port, hud, screen, unlisten } = setup({ autoSpawn: false });
  t.after(unlisten);
  screen.spawn();
  hud.battle.over = true;
  screen.update(1 / 60);
  assert.equal(screen.isOpen, false);
  assert.equal(port.cancels, 1);
  port.await();
  assert.equal(screen.isOpen, false, "and doesn't open again");
});

test("spectators never open the spawn screen or submit a choice", (t) => {
  const { port, hud, controls, screen, unlisten } = setup({ human: { spectator: true } });
  t.after(unlisten);
  assert.equal(screen.isOpen, false);
  assert.equal(hud.layer, null);
  assert.equal(screen.spawn(), null);
  port.await();
  assert.equal(screen.isOpen, false);
  assert.deepEqual(port.requests, []);
  assert.deepEqual(port.viewShots, []);
  assert.equal(controls.locks, 0);
});

test("a late spectator welcome blocks clicks immediately and closes the existing spawn screen", (t) => {
  const { port, hud, controls, screen, unlisten } = setup();
  t.after(unlisten);
  assert.equal(screen.isOpen, true);
  port.human.spectator = true;
  assert.equal(screen.spawn(), null, "welcome can arrive between display and click");
  assert.deepEqual(port.requests, []);
  assert.equal(controls.locks, 0);
  screen.update(1 / 60);
  assert.equal(screen.isOpen, false);
  assert.equal(hud.layer, null);
  port.await();
  assert.equal(screen.isOpen, false, "later world events cannot reopen it");
});
