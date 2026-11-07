import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import * as THREE from "three";
import { Ordnance } from "../src/ordnance.js";
import { ProjectilePresentation } from "../src/runtime/network-combat-projectile.js";
import { boltLook } from "../src/blaster-bolts.js";
import { sniperLook } from "../src/sniper-trace.js";
import { OrdnanceFlag as FL } from "../src/presentation/ordnance-layout.js";
import { presentationLoaded } from "./helpers/presentation.js";
import { run, surroundings } from "./helpers/ordnance-scenario.js";

// The rounds a seeded battle draws, against what the JavaScript this phase
// replaced drew for the same rounds (recorded before it was deleted): every
// batch's quads, the lights asked for, the sniper rounds handed to the
// trace, and where each mesh and trail was moved, hashed for each frame.
const golden = JSON.parse(fs.readFileSync(new URL("./fixtures/ordnance/golden.json", import.meta.url), "utf8"));
const MODES = { classic: [false, false], bolts: [true, false], sniper: [false, true], both: [true, true] };

// A page's ordnance as the scenario's driver sees it.
function driver([bolts, sniper]) {
  const sur = surroundings(THREE);
  const ordnance = new Ordnance({
    scene: new THREE.Scene(), collision: { cast: () => null }, lights: sur.lights, texture: sur.texture, presentation: presentationLoaded,
    effects: { play: () => ({ moves: [], move(p, q, v) { this.moves.push([p.toArray(), q ? q.toArray() : null, v ? v.toArray() : null]); }, stop() {} }) },
    model: () => () => new THREE.Object3D(),
  });
  ordnance.sniperTrace = () => sur.trace;
  let projectiles;
  return {
    ordnance, capture: sur.capture,
    attach(s) { boltLook.enhanced = bolts; sniperLook.enhanced = sniper; projectiles = new ProjectilePresentation(ordnance, s.definition); },
    receive(event) { projectiles.receive(event); },
    present(f) {
      boltLook.enhanced = bolts; sniperLook.enhanced = sniper;
      ordnance.bolts = f.world; ordnance.rounds = f.rounds; ordnance.beams = f.beams; ordnance.grenades = f.grenades;
      ordnance.present(f.alpha, f.clock, {
        draw() {
          projectiles.present(f.clock);
          for (const b of f.linear) {
            // CombatPresentation.present: the gun as drawn, taken at the first draw.
            const muzzle = b.visualMuzzle ? b.visualMuzzle() : null;
            b.visualMuzzle = null;
            ordnance.draw.bolt(b, 1, f.clock, muzzle, FL.RESOLVE_SHOWN);
          }
          for (const b of f.predicted) ordnance.draw.bolt(b, 1, null);
        },
        settle() {},
      });
    },
    objects(f) {
      return [...f.rounds, ...f.world, ...f.linear, ...f.predicted, ...f.grenades, ...projectiles.objects.values()];
    },
  };
}

for (const [mode, flags] of Object.entries(MODES)) {
  test(`${mode} looks draw the recorded quads, lights, touches and moves for every seed`, () => {
    const before = [boltLook.enhanced, sniperLook.enhanced];
    try {
      for (const seed of golden.seeds) {
        const got = run(THREE, driver(flags), { seed, frames: 90, bolts: 12 });
        const want = golden.modes[mode][seed];
        for (let k = 0; k < want.length; k++) assert.deepEqual(got[k], want[k], `${mode} seed ${seed} frame ${k}`);
      }
    } finally { [boltLook.enhanced, sniperLook.enhanced] = before; }
  });
}

// The worst a long load, a pause or a crowded fight can ask of the stores:
// more rounds than a frame holds, more replicated projectiles than slots,
// and more events between two frames than the command block. Nothing throws;
// what does not fit is not drawn, and is counted once.
test("a crowd of rounds and projectiles beyond every store degrades without throwing", async () => {
  const { OrdnanceLimits: L, OrdnanceHeader: H } = await import("../src/presentation/ordnance-layout.js");
  const before = [boltLook.enhanced, sniperLook.enhanced], warned = [], warn = console.warn;
  console.warn = (...args) => warned.push(args.join(" "));
  try {
    boltLook.enhanced = false; sniperLook.enhanced = false;
    const sur = surroundings(THREE);
    const ordnance = new Ordnance({
      scene: new THREE.Scene(), collision: { cast: () => null }, lights: sur.lights, texture: sur.texture, presentation: presentationLoaded,
      effects: { play: () => ({ move() {}, stop() {} }) }, model: () => () => new THREE.Object3D(),
    });
    const defs = {
      rifle: { Velocity: 100, LifeSpan: 2, LaserLength: 3, LaserTexture: "red", GlowLength: 6, LightRadius: 3 },
      rocket: { base: "bullet", GeometryName: "rocket", TrailEffect: "trail", LightRadius: 2, Velocity: 50, LifeSpan: 3 },
    };
    const projectiles = new ProjectilePresentation(ordnance, () => defs.rocket);
    // More events than the command block holds, and more projectiles than slots, before any frame.
    const total = L.NET_SLOTS + 900;
    for (let id = 1; id <= total; id++) {
      projectiles.receive({ schema: 6, kind: "combat", id, generation: 1, tick: 10, expiryTick: 28, definitionId: 0, type: "round", life: 1,
        position: [id, 0, 0], velocity: [10, 0, 0], quaternion: [0, 0, 0, 1], end: [id, 0, 1] });
    }
    assert.equal(projectiles.objects.size, L.NET_SLOTS, "every slot held, the rest refused");
    // More rounds than a frame's records, all of one class, with meshes and trails.
    const rounds = [];
    for (let i = 0; i < L.RECORDS + 500; i++) {
      const at = new THREE.Vector3(i, 0, 0);
      rounds.push({ def: defs.rocket, previous: at.clone(), position: at, velocity: new THREE.Vector3(1, 0, 0), quaternion: new THREE.Quaternion(),
        launchPosition: at.clone(), visualMuzzle: () => new THREE.Vector3(i, 1, 0), mesh: new THREE.Object3D(), trail: { move() {} } });
    }
    const bolts = [];
    for (let i = 0; i < L.QUADS_PER_HANDLE + 300; i++) {
      bolts.push({ id: "w" + i, def: defs.rifle, position: new THREE.Vector3(i, 0, 0), direction: new THREE.Vector3(0, 0, 1), speed: 100, life: 2, flown: 5, drawn: 3, merge: 8, offset: null });
    }
    ordnance.rounds = rounds; ordnance.bolts = bolts;
    for (let frame = 0; frame < 3; frame++) {
      assert.doesNotThrow(() => ordnance.present(1, 11 + frame, { draw: () => projectiles.present(11 + frame), settle() {} }));
      // The ended projectiles went with the kernel's own expiry; the rest are still held.
      assert.ok(projectiles.objects.size <= L.NET_SLOTS);
    }
    const header = ordnance.draw.header;
    assert.equal(header[H.STATUS], 0);
    assert.ok(header[H.DROPPED_QUADS] > 0 || ordnance.draw.dropped > 0, "work that did not fit was counted");
    assert.equal(warned.length, 1, "and reported once");
    // A burst of replicated events with no frame between them past the block never loses one.
    for (let id = total + 1; id <= total + L.NET_COMMANDS + 10; id++) {
      projectiles.receive({ schema: 6, kind: "combat", id, generation: 1, tick: 20, expiryTick: 38, definitionId: 0, type: "round", life: 1,
        position: [0, 0, 0], velocity: [0, 0, 0], quaternion: [0, 0, 0, 1], end: [0, 0, 1] });
    }
    assert.doesNotThrow(() => ordnance.present(1, 30, { draw: () => projectiles.present(30), settle() {} }));
  } finally { console.warn = warn; [boltLook.enhanced, sniperLook.enhanced] = before; }
});

// The module's memory is shared by every presentation phase: another phase
// growing it replaces the views of it, and a write to a stale one is lost.
test("a frame survives the shared memory growing between its records and its call", () => {
  const before = [boltLook.enhanced, sniperLook.enhanced];
  try {
    boltLook.enhanced = false; sniperLook.enhanced = false;
    const sur = surroundings(THREE);
    const ordnance = new Ordnance({ scene: new THREE.Scene(), collision: { cast: () => null }, lights: sur.lights, texture: sur.texture, presentation: presentationLoaded,
      effects: { play: () => ({ move() {}, stop() {} }) }, model: () => () => new THREE.Object3D() });
    const def = { Velocity: 100, LifeSpan: 2, LaserLength: 3, LaserTexture: "red", GlowLength: 6, LightRadius: 3 };
    const bolt = { id: "w0", def, position: new THREE.Vector3(), direction: new THREE.Vector3(0, 0, 1), speed: 100, life: 2, flown: 5, drawn: 3, merge: 8, offset: null };
    ordnance.bolts = [bolt];
    ordnance.present(1, null, { draw() { presentationLoaded.memory.grow(1); }, settle() {} });
    assert.ok([...ordnance.renderer.batches.values()].some((b) => b.count > 0), "the bolt written before the growth was still drawn");
  } finally { [boltLook.enhanced, sniperLook.enhanced] = before; }
});
