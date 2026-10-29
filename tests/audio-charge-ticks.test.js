import test from "node:test";
import assert from "node:assert/strict";
import { SceneSounds } from "../src/audio/scene.js";

// The time bomb's tick and other soldiers' charge sounds (src/audio/scene.js),
// with made-up sound data (no retail assets).
function rig(game) {
  const played = [];
  const audio = {
    game,
    data: {
      classSounds: (cls) => ({ timebomb: { TickSound: "tick", TickSoundPitch: "0.5" }, bowcaster: { ChargeSound: "hum", ChargeSoundPitch: "0.05" } })[cls] || null,
      sound: () => null,
    },
    playClass: (cls, key, options) => {
      const voice = { cls, key, options, pitch: null, stopped: false, setAbsolutePitch(p) { this.pitch = p; }, stop() { this.stopped = true; } };
      played.push(voice);
      return voice;
    },
  };
  return { scene: new SceneSounds(audio), played };
}

test("a time bomb ticks as its whole seconds change, falling in pitch to TickSoundPitch", () => {
  const bomb = { def: { class: "timebomb", base: "mine", LifeSpan: 5 }, life: 5, position: { x: 1, y: 2, z: 3 }, mine: true, landed: false };
  const { scene, played } = rig({ items: { items: [bomb], views: new Set() } });
  for (let i = 0; i < 300 && bomb.life > 0; i++) {
    bomb.life -= 1 / 60;
    scene.updateItems(1 / 60);
  }
  const ticks = played.filter((v) => v.key === "TickSound");
  // Placed (twice: its 5 s, then 4.98), then at 3, 2, 1 and 0 whole seconds left.
  assert.equal(ticks.length, 6);
  assert.equal(ticks[0].pitch, 1, "the first at full pitch");
  assert.ok(Math.abs(ticks.at(-1).pitch - 0.6) < 0.01, "under a second left: 0.2 x 1 + 0.8 x 0.5");
  assert.ok(ticks.every((v, i) => i === 0 || v.pitch < ticks[i - 1].pitch), "each lower");
  // A land mine (no LifeSpan) and a power-up never tick.
  const quiet = rig({ items: { items: [{ def: { class: "timebomb", base: "mine" }, life: Infinity, position: {} }, { def: { class: "timebomb", base: "powerupitem", LifeSpan: 30 }, life: 30, position: {} }], views: new Set() } });
  for (let i = 0; i < 120; i++) quiet.scene.updateItems(1 / 60);
  assert.equal(quiet.played.length, 0);
});

test("a time bomb shown online ticks by its own count from when it was first seen", () => {
  const view = { def: { class: "timebomb", base: "mine", LifeSpan: 5 }, position: { x: 0, y: 0, z: 0 } };
  const items = { items: [], views: new Set([view]) };
  const { scene, played } = rig({ items });
  for (let i = 0; i < 150; i++) scene.updateItems(1 / 60);
  assert.equal(played.length, 4, "at once, then at 4, 3 and 2 s left");
  items.views.delete(view);
  scene.updateItems(1 / 60);
  assert.equal(scene.timers.size, 0);
});

test("another soldier's charge plays its ChargeSound at him until he lets go", () => {
  const weapon = { charging: false, held: 0, def: { class: "bowcaster", base: "cannon" }, levels: [{ MaxPressedTime: 1.75 }] };
  const bot = { removed: false, soldier: { position: { x: 5, y: 0, z: 0 } }, combat: { dead: false, arsenal: { current: () => weapon } } };
  const own = { dead: false, arsenal: { current: () => ({ ...weapon, charging: true }) } };
  const player = { removed: false, soldier: { position: {} }, combat: own };
  const { scene, played } = rig({ combat: own, units: { all: () => [player, bot] } });
  scene.updateCharges();
  assert.equal(played.length, 0, "nothing until he charges; his own is the player's");
  weapon.charging = true;
  scene.updateCharges();
  assert.equal(played.length, 1);
  assert.equal(played[0].key, "ChargeSound");
  assert.equal(played[0].options.follow(), bot.soldier.position);
  assert.equal(played[0].pitch, 0.2, "starts at ChargeSoundPitch, held to 0.2");
  weapon.held = 1.75;
  scene.updateCharges();
  assert.equal(played[0].pitch, 0.4, "full: the original game's 0.4 ceiling");
  weapon.charging = false;
  scene.updateCharges();
  assert.equal(played[0].stopped, true);
  // A launcher's charge makes no sound for anyone but its owner.
  weapon.charging = true; weapon.def = { class: "bowcaster", base: "launcher" };
  scene.updateCharges();
  assert.equal(played.length, 1);
});
