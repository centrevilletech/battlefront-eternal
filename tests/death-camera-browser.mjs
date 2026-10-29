// The death camera in a headless browser (docs/formats.md, "Death camera"),
// with the Classic AI and unit= (so he comes back by himself 4 s after each
// death). The battle runs in the world worker, stepped in lockstep with the
// page's frames (tests/battle-page.mjs); the deaths are set up there. The
// world's camera (the one the Classic scheduling reads) is the original's,
// stepped in the tick:
//   - killed by a soldier standing behind him to the left, it rises to look
//     down on the body, holds, and from 1.5 s turns around to look over the
//     body at the killer, framing him by 2 s and following him;
//   - killed by no one (RESPAWN) in first person, it starts from his eyes,
//     shows his body, looks down on it and stays;
//   - killed by a bot driving an AAT behind him, it turns to the AAT;
//   - killed with the tank he drives, it starts from the tank's view and
//     turns to whoever destroyed it;
//   - a droideka gets none: its chase camera stays on the wreck;
//   - a jet trooper killed in the air: it follows the body down.
// The drawn camera is the page's own (src/death-shot.js), from the body and
// the killer as drawn:
//   - Enhanced (the default) holds on the body, swings round it to look over
//     it at the killer with both in frame, then frames the killer close;
//   - Classic draws the world camera's framing;
//   - at 60 and 144 frames a second and at uneven frames it moves without
//     reversals or single-frame spikes, its acceleration bounded the same,
//     on the killer the world named.
// Frame sequences for the soldier, vehicle and no-killer deaths go to
// DEATHCAM_SHOTS (default /tmp/bf-punch/deathcam/<case>/), with each tick's
// cameras in timeline.json. Needs the extracted maps and a Vite server that
// is cross-origin isolated (the worker's shared ring).
//   PLAYWRIGHT_MODULE=.../playwright/index.mjs node tests/death-camera-browser.mjs
import assert from "node:assert/strict";
import fs from "node:fs/promises";
import * as THREE from "three";
import { launchBrowser, openBattle } from "./battle-page.mjs";

const BASE = process.env.DEATHCAM_URL || "http://127.0.0.1:18449";
const MAP = process.env.DEATHCAM_MAP || "nab2";
const OUT = process.env.DEATHCAM_SHOTS || "/tmp/bf-punch/deathcam";
const SHOTS = process.env.DEATHCAM_SHOTS !== "0";
// The moments drawn, in seconds after the death.
const TIMES = [0, 0.25, 0.5, 1, 1.5, 1.65, 1.75, 2, 2.5, 3, 3.9];

if (process.env.DEATHCAM_CHROMIUM) process.env.CHROMIUM = process.env.DEATHCAM_CHROMIUM;
const browser = await launchBrowser();
const failures = [];

// A battle with the checks' helpers on the page and in the world.
async function open(query) {
  const battle = await openBattle(browser, { base: BASE, query, viewport: { width: 800, height: 450 },
    settings: { shadows: "off", antialiasing: "off", upscaler: "off" } });
  await battle.page.evaluate(HELPERS);
  await battle.world.evaluate(WORLD);
  return battle;
}

// The drawn camera frame to frame (samples of { t, p, q }): the largest
// change of velocity (m/s2) and of turning rate (deg/s2), velocity
// reversals, and single-frame spikes.
function motion(samples) {
  const v = [], w = [];
  for (let i = 1; i < samples.length; i++) {
    const dt = samples[i].t - samples[i - 1].t;
    const a = samples[i - 1], b = samples[i];
    v.push({ dt, v: new THREE.Vector3(...b.p).sub(new THREE.Vector3(...a.p)).divideScalar(dt) });
    const turn = new THREE.Quaternion(...b.q).multiply(new THREE.Quaternion(...a.q).invert());
    if (turn.w < 0) turn.set(-turn.x, -turn.y, -turn.z, -turn.w);
    const s = Math.hypot(turn.x, turn.y, turn.z);
    w.push(s > 1e-12 ? new THREE.Vector3(turn.x, turn.y, turn.z).multiplyScalar((2 * Math.atan2(s, turn.w)) / s / dt) : new THREE.Vector3());
  }
  let accel = 0, spin = 0, reversals = 0, spikes = 0, step = 0;
  for (let i = 1; i < v.length; i++) {
    const dt = (v[i].dt + v[i - 1].dt) / 2;
    accel = Math.max(accel, v[i].v.distanceTo(v[i - 1].v) / dt);
    spin = Math.max(spin, THREE.MathUtils.radToDeg(w[i].distanceTo(w[i - 1]) / dt));
    if (v[i].v.length() > 0.25 && v[i - 1].v.length() > 0.25 && v[i].v.dot(v[i - 1].v) < 0) reversals++;
    if (w[i].length() > 0.05 && w[i - 1].length() > 0.05 && w[i].dot(w[i - 1]) < 0) reversals++;
  }
  for (let i = 0; i < v.length; i++) step = Math.max(step, v[i].v.length() * v[i].dt);
  for (let i = 1; i + 1 < v.length; i++) {
    const speed = v[i].v.length();
    if (speed > 0.5 && speed > 3 * Math.max(v[i - 1].v.length(), v[i + 1].v.length(), 0.05)) spikes++;
  }
  return { accel, spin, reversals, spikes, step };
}
const show = (m) => `${m.accel.toFixed(1)} m/s2, ${m.spin.toFixed(0)} deg/s2, step ${m.step.toFixed(3)} m, ${m.reversals} reversals, ${m.spikes} spikes`;
// Degrees between a camera's view ([x, y, z, qx, qy, qz, qw]) and the line to a point.
const offView = (view, point) => (point ? THREE.MathUtils.radToDeg(new THREE.Vector3(0, 0, -1).applyQuaternion(new THREE.Quaternion(...view.slice(3)))
  .angleTo(new THREE.Vector3(...point).sub(new THREE.Vector3(...view.slice(0, 3))))) : null);

// In the page: its frames, and what the drawn camera shows.
const HELPERS = () => {
  const b = window.__bf;
  const T = b.THREE;
  const scratch = new T.Vector3();
  // n frames of a tick each (drawn if draw).
  window.__run = async (n, draw = false) => {
    for (let i = 0; i < n; i++) await b.lockstep(1 / 60, draw);
  };
  // On to the start of the next tick, where a 60 Hz frame lands.
  window.__align = async () => {
    const phase = b.client.playerAlpha;
    if (phase > 1e-9) await b.lockstep((1 - phase) / 60, false);
  };
  // Degrees between a camera's view and the line to a point.
  window.__off = (camera, point) => T.MathUtils.radToDeg(new T.Vector3(0, 0, -1).applyQuaternion(camera.quaternion).angleTo(scratch.subVectors(point, camera.position)));
  window.__body = () => b.soldier.position.clone().add(new T.Vector3(0, 0.8, 0));
  // The drawn camera as this frame left it, at the tick the page shows.
  window.__drawn = () => ({ tick: b.worldClock.tick, drawn: [...b.camera.position.toArray(), ...b.camera.quaternion.toArray()], known: b.deathShot.known });
  // The killer as drawn, by his unit's id (the page has no deathKiller of
  // its own: the world names him).
  window.__killerId = null;
  window.__drawnKiller = () => {
    const unit = window.__killerId !== null ? b.units.byId(window.__killerId) : null;
    if (!unit) return null;
    return (unit.actor?.root?.position ?? unit.presentation.position).clone().add(new T.Vector3(0, 1.7, 0));
  };
  // The drawn camera each frame, the frames cut by pattern (a list of dts).
  window.__drive = async (seconds, pattern) => {
    const samples = [];
    let t = 0, i = 0;
    while (t < seconds - 1e-9) {
      const dt = pattern[i++ % pattern.length];
      await b.lockstep(dt, false);
      t += dt;
      const killer = window.__drawnKiller(), body = window.__body();
      samples.push({ t, active: b.deathView.active, p: b.camera.position.toArray(), q: b.camera.quaternion.toArray(), fov: b.camera.fov,
        killerOff: killer ? window.__off(b.camera, killer) : null, bodyOff: window.__off(b.camera, body), known: b.deathShot.known });
    }
    return samples;
  };
};

// In the world (the worker's game, self.__bf): what the checks set up, and
// its camera each tick while self.__dc.samples is a list.
const WORLD = () => {
  const b = self.__bf;
  const T = b.THREE;
  const scratch = new T.Vector3();
  const off = (camera, point) => T.MathUtils.radToDeg(new T.Vector3(0, 0, -1).applyQuaternion(camera.quaternion).angleTo(scratch.subVectors(point, camera.position)));
  const pitch = (camera) => T.MathUtils.radToDeg(Math.asin(new T.Vector3(0, 0, -1).applyQuaternion(camera.quaternion).y));
  // The body's middle, and the killer's eyes as the death camera sees them.
  const body = () => b.soldier.position.clone().add(new T.Vector3(0, 0.8, 0));
  const killer = () => {
    const k = b.deathKiller;
    if (!k) return null;
    if (k.vehicle) return k.vehicle.alive ? k.vehicle.seatOrigin(0, new T.Vector3()) : null;
    const unit = k.character ? k.character.unit : k.unit;
    return unit?.alive ? unit.origin(new T.Vector3()) : null;
  };
  const dc = self.__dc = {
    off, pitch, body, killer, samples: null,
    // An enemy soldier on foot (the Classic AI's), nearest first.
    enemy: () => [...b.units.living()]
      .filter((u) => !u.human && u.soldier && !u.walker && !u.seat && b.units.relation(u.team, b.playerTeam) < 0)
      .sort((a, c) => a.position.distanceTo(b.soldier.position) - c.position.distanceTo(b.soldier.position))[0] ?? null,
    // A unit put down at a point, facing yaw.
    place(unit, x, y, z, yaw) {
      unit.soldier.spawn(x, y + 1, z, yaw);
      unit.presentation?.reset();
      unit.tickCount += 1;
    },
    // The player put down at a point, facing yaw, as a respawn would.
    putPlayer(x, y, z, yaw) {
      b.soldier.spawn(x, y + 1, z, yaw);
      b.presentation.reset();
      b.playerUnit.tickCount += 1;
    },
    // The world's camera now, and how it frames the body and the killer.
    sample() {
      const sim = b.simulationCamera, k = killer(), at = body();
      return {
        tick: b.worldClock.tick, active: b.deathView.active, alive: b.playerUnit.alive,
        sim: [...sim.position.toArray(), ...sim.quaternion.toArray()],
        pitch: pitch(sim), bodyOff: off(sim, at), killerOff: k ? off(sim, k) : null,
        killerNdc: k ? k.clone().project(sim).toArray() : null, killer: k?.toArray() ?? null, body: at.toArray(), fov: sim.fov,
      };
    },
  };
  const step = b.step;
  b.step = function (command) {
    step.call(this, command);
    dc.samples?.push(dc.sample());
  };
};

// A tick's world sample with the frame drawn at it: t in seconds after the
// death (at tick0), and how the drawn camera frames the body and the killer.
function merge(world, frame, tick0) {
  if (!world) throw new Error(`no world sample for tick ${frame.tick}`);
  return { ...world, t: (world.tick - tick0) / 60, drawn: frame.drawn, known: frame.known,
    drawnKillerOff: offView(frame.drawn, world.killer), drawnBodyOff: offView(frame.drawn, world.body) };
}
// From start() (its sample of the world now) to recorded() (the world's
// sample of every tick since, by tick), the world keeps a sample a tick.
async function recorded(world) {
  const samples = await world.evaluate(() => {
    const samples = self.__dc.samples;
    self.__dc.samples = null;
    return samples;
  });
  return new Map(samples.map((s) => [s.tick, s]));
}
const start = (world) => world.evaluate(() => {
  self.__dc.samples = [];
  return self.__dc.sample();
});

// Frames of a tick each from `from` to `to` (in 60ths of a second after the
// death), drawing the last one into a shot when asked; the drawn camera of
// every frame.
async function advance(page, from, to, draw) {
  return page.evaluate(async ({ from, to, draw }) => {
    const b = window.__bf;
    const frames = [];
    for (let tick = from + 1; tick <= to; tick++) {
      await b.lockstep(1 / 60, draw && tick === to);
      frames.push(window.__drawn());
    }
    if (draw && to === from) await b.lockstep(0, true);
    return { frames, image: draw ? b.renderer.domElement.toDataURL("image/png") : null };
  }, { from, to, draw });
}

// The 4 s after a death: shots at TIMES into OUT/name, every tick sampled.
async function sequence({ page, world }, name, seconds = 4) {
  const dir = `${OUT}/${name}`;
  if (SHOTS) await fs.mkdir(dir, { recursive: true });
  const first = await start(world);
  const frames = [await page.evaluate(() => window.__drawn())];
  let tick = 0;
  for (const time of TIMES.filter((t) => t <= seconds)) {
    const to = Math.round(time * 60);
    const { frames: more, image } = await advance(page, tick, to, SHOTS);
    frames.push(...more);
    tick = to;
    if (image) await fs.writeFile(`${dir}/t${time.toFixed(2)}.png`, Buffer.from(image.split(",")[1], "base64"));
  }
  const end = Math.round(seconds * 60);
  if (tick < end) frames.push(...(await advance(page, tick, end, false)).frames);
  const ticks = await recorded(world);
  ticks.set(first.tick, first);
  const samples = frames.map((frame) => merge(ticks.get(frame.tick), frame, first.tick));
  if (SHOTS) await fs.writeFile(`${dir}/timeline.json`, JSON.stringify(samples));
  return samples;
}

const at = (samples, t) => samples.reduce((best, s) => (Math.abs(s.t - t) < Math.abs(best.t - t) ? s : best));
const moved = (a, b) => Math.hypot(a.sim[0] - b.sim[0], a.sim[1] - b.sim[1], a.sim[2] - b.sim[2]);
function report(name, samples) {
  const row = (t) => {
    const s = at(samples, t);
    return `${t}s pitch ${s.pitch.toFixed(1)} body ${s.bodyOff.toFixed(1)} killer ${s.killerOff?.toFixed(1) ?? "-"}`;
  };
  console.log(`${name}: ${[0.5, 1.5, 1.75, 2, 3.9].map(row).join(" | ")}`);
}
// Waits out the textures for the shots: a few drawn frames, slowly.
const settle = (page) => page.evaluate(async () => {
  for (let i = 0; i < 20; i++) {
    await new Promise((r) => setTimeout(r, 50));
    await window.__bf.lockstep(1 / 60, true);
  }
});
// Alive (frames until he is), from the start of a tick, and too tough for
// the bots until the scripted death (any death takes him past his health).
async function ready({ page, world }) {
  await page.evaluate(async () => {
    const b = window.__bf;
    for (let i = 0; i < 400 && !b.playerUnit.alive; i++) await b.lockstep(1 / 60, false);
    await window.__align();
  });
  return world.evaluate(() => {
    self.__bf.combat.health = 1e9;
    return self.__bf.playerUnit.alive;
  });
}
// Until he is back (unit= brings him back about 4 s after the death).
async function back({ page, world }) {
  await page.evaluate(async () => {
    const b = window.__bf;
    for (let i = 0; i < 120 && !b.playerUnit.alive; i++) await b.lockstep(1 / 60, false);
  });
  return world.evaluate(() => {
    const b = self.__bf;
    return { alive: b.playerUnit.alive, active: b.deathView.active, distance: b.simulationCamera.position.distanceTo(b.combat.pivot) };
  });
}

async function check(name, fn) {
  try {
    await fn();
  } catch (error) {
    failures.push(`${name}: ${error.stack}`);
  }
}

// In the world: whether he is alive (and then too tough for the bots until
// the scripted death); the nearest enemy soldier put 20 m behind him to the
// left, too tough to die meanwhile (his id); his death at that soldier's
// hands.
const toughen = () => {
  const w = self.__bf;
  if (w.playerUnit.alive) w.combat.health = 1e9;
  return w.playerUnit.alive;
};
const placeKiller = () => {
  const w = self.__bf, p = w.soldier.position, bot = self.__dc.enemy();
  if (!bot) return null;
  const yaw = w.soldier.aimYaw + (2 * Math.PI) / 3;
  self.__dc.place(bot, p.x + Math.sin(yaw) * 20, p.y, p.z + Math.cos(yaw) * 20, yaw + Math.PI);
  bot.combat.health = 1e9;
  return bot.id;
};
const killBy = (id) => {
  const w = self.__bf;
  w.combat.damage(Infinity, Infinity, null, w.units.byId(id));
  return w.combat.dead;
};
// A death at a soldier's hands, its drawn camera sampled every frame of the
// pattern for 3.9 s: { id, named, samples }, named the killer the page's
// death camera turns to.
async function drawnDeath({ page, world }, pattern) {
  for (let i = 0; i < 40 && !(await world.evaluate(toughen)); i++) await page.evaluate(() => window.__drive(0.5, [1 / 60]));
  await page.evaluate(() => window.__drive(0.5, [1 / 60]));
  const id = await world.evaluate(placeKiller);
  assert.notEqual(id, null, "an enemy soldier to be the killer");
  await page.evaluate((id) => { window.__killerId = id; }, id);
  await page.evaluate(() => window.__drive(0.25, [1 / 60]));
  assert.ok(await world.evaluate(killBy, id), "dead");
  const samples = await page.evaluate((pattern) => window.__drive(3.9, pattern), pattern);
  const named = await page.evaluate(() => window.__bf.deathView.killerUnit);
  // Back in a body.
  await page.evaluate(() => window.__drive(2.5, [1 / 60]));
  return { id, named, samples: samples.filter((s) => s.active) };
}

try {
  // ------------------------------------------------------------ Theed, Classic AI
  {
    const battle = await open(`map=${MAP}&unit=rep_inf_clone_trooper&ai=classic&seed=1`);
    const { page, world, errors } = battle;
    // The first wave lands about 10 s in.
    await page.evaluate(() => window.__run(720));
    await settle(page);

    // A soldier behind him to the left, 20 m off: the camera has to turn
    // around the body to find him.
    await check("soldier killer", async () => {
      const ready0 = await ready(battle);
      const id = await world.evaluate(() => {
        const b = self.__bf, dc = self.__dc;
        const bot = dc.enemy();
        const p = b.soldier.position;
        const yaw = b.soldier.aimYaw + (2 * Math.PI) / 3;
        dc.place(bot, p.x + Math.sin(yaw) * 20, p.y, p.z + Math.cos(yaw) * 20, yaw + Math.PI);
        // He lives through the shot, so there is someone to follow.
        bot.combat.health = 1e9;
        return bot.id;
      });
      // A tick so the cameras and the bot's aim settle where they are.
      await page.evaluate(() => window.__run(1));
      const setup = await world.evaluate((id) => {
        const b = self.__bf, bot = b.units.byId(id), p = b.soldier.position;
        const before = { sim: b.simulationCamera.position.clone(), quaternion: b.simulationCamera.quaternion.clone() };
        const scheduler = b.ai.classic.ai.cameras()[0] === b.simulationCamera;
        const living = b.playerUnit.alive;
        b.combat.damage(Infinity, Infinity, null, bot);
        return {
          living, alive: b.playerUnit.alive, active: b.deathView.active, killer: b.deathKiller?.unit === bot, character: b.deathKiller?.character === bot.character,
          from: b.deathView.position.distanceTo(before.sim) < 1e-9 && b.deathView.quaternion.angleTo(before.quaternion) < 1e-9,
          scheduler, distance: bot.position.distanceTo(p),
        };
      }, id);
      const samples = await sequence(battle, "soldier");
      report("soldier killer", samples);
      const respawn = await back(battle);
      assert.deepEqual(errors, []);
      assert.ok(ready0 && setup.living, "alive until the scripted death");
      assert.equal(setup.alive, false);
      assert.ok(setup.active && setup.killer && setup.character, "the death camera starts at once, on the killer's Character");
      assert.ok(setup.from, "from the world's camera as it stood");
      assert.ok(setup.scheduler, "the Classic scheduling reads the world's camera");
      // It rises to look down on the body, about 35 degrees.
      const down = at(samples, 0.6);
      assert.ok(Math.abs(down.pitch + 35) < 3 && down.bodyOff < 2, `looking down at the body at 0.6 s (${down.pitch.toFixed(1)}, ${down.bodyOff.toFixed(1)})`);
      assert.ok(down.sim[1] > samples[0].sim[1] + 1, "above where it was");
      assert.ok(moved(at(samples, 1.0), at(samples, 1.5)) < 1e-6, "still from 1 s to 1.5 s");
      assert.ok(at(samples, 1.5).killerOff > 60, "the killer was well out of view");
      // From 1.5 s it turns to him; by 2 s he is in the middle of the view.
      const framed = at(samples, 2.0);
      assert.ok(framed.killerOff < 3, `framed by 2 s (${framed.killerOff.toFixed(2)})`);
      assert.ok(framed.killerNdc.slice(0, 2).every((v) => Math.abs(v) < 0.2) && framed.killerNdc[2] < 1, "in the middle of the view");
      // (A tick behind him as he runs: it steps before the bots move.)
      for (const s of samples.filter((s) => s.t >= 2.2 && s.active)) assert.ok(s.killerOff < 2, `following him at ${s.t.toFixed(2)} s (${s.killerOff.toFixed(2)})`);
      // The drawn camera (Enhanced): on the body as it holds, over it at him
      // with both in frame, then on him; smoothly.
      assert.ok(at(samples, 1.0).drawnBodyOff < 15, `the drawn view holds on the body (${at(samples, 1.0).drawnBodyOff.toFixed(1)})`);
      const two = at(samples, 2.6);
      assert.ok(two.drawnKillerOff < 17 && two.drawnBodyOff < 20, `both in the drawn view at 2.6 s (${two.drawnKillerOff.toFixed(1)}, ${two.drawnBodyOff.toFixed(1)})`);
      for (const s of samples.filter((s) => s.t >= 3.4 && s.active)) assert.ok(s.drawnKillerOff < 6, `drawn on him at ${s.t.toFixed(2)} s (${s.drawnKillerOff.toFixed(2)})`);
      const drawn = motion(samples.filter((s) => s.active).map((s) => ({ t: s.t, p: s.drawn.slice(0, 3), q: s.drawn.slice(3) })));
      assert.ok(drawn.reversals === 0 && drawn.spikes === 0 && drawn.accel < 60, `drawn smoothly: ${show(drawn)}`);
      // Its field of view: the chase camera's after one step toward a dead
      // soldier's 48 degrees, then held.
      const across = (v) => 2 * Math.atan(Math.tan((v * Math.PI) / 360) * (800 / 450));
      const held = samples.filter((s) => s.active && s.t > 0).map((s) => s.fov);
      const want = 0.837758 + (across(samples[0].fov) - 0.837758) * Math.exp(-25 / 60);
      assert.ok(held.every((v) => v === held[0]) && Math.abs(across(held[0]) - want) < 1e-6, `field of view ${across(held[0])} vs ${want}`);
      assert.ok(respawn.alive && !respawn.active, "back in a body, the chase camera again");
      assert.ok(respawn.distance < 6, `the chase camera on the new body (${respawn.distance.toFixed(2)} m)`);
    });

    // No killer (the pause menu's RESPAWN), in first person.
    await check("no killer", async () => {
      await page.evaluate(() => window.__run(30));
      await settle(page);
      const ready0 = await ready(battle);
      await page.evaluate(async () => {
        window.__bf.setPerspective(true);
        await window.__run(2);
      });
      const before = await world.evaluate(() => {
        const b = self.__bf;
        return { eye: b.combat.eye(b.soldier.position, new b.THREE.Vector3()).toArray(), sim: b.simulationCamera.position.toArray() };
      });
      // RESPAWN goes to the world with the next tick's command.
      const bodyDrawn = await page.evaluate(async () => {
        const b = window.__bf;
        b.spawnPort.suicide();
        await window.__run(1);
        return b.actor.root.children.some((child) => child.isObject3D && child.layers.test(b.camera.layers));
      });
      const setup = await world.evaluate(({ eye, sim }) => {
        const b = self.__bf, V = b.THREE.Vector3, before = new V(...sim);
        return {
          dead: b.combat.dead, active: b.deathView.active, killer: b.deathKiller ? "someone" : null,
          fromEyes: before.distanceTo(new V(...eye)) < 0.05 && b.deathView.previousPosition.distanceTo(before) < 0.05,
        };
      }, before);
      const samples = await sequence(battle, "none");
      report("no killer", samples);
      const respawn = await back(battle);
      await page.evaluate(() => window.__bf.setPerspective(false));
      assert.deepEqual(errors, []);
      assert.ok(ready0 && setup.dead && setup.active, "a death camera");
      assert.equal(setup.killer, null, "no one to turn to");
      assert.ok(setup.fromEyes, "starting from his eyes");
      assert.ok(bodyDrawn, "his body is drawn, though he was in first person");
      const end = at(samples, 3.9);
      assert.ok(Math.abs(end.pitch + 35) < 3 && end.bodyOff < 2, `still looking down on the body (${end.pitch.toFixed(1)}, ${end.bodyOff.toFixed(1)})`);
      assert.ok(moved(at(samples, 1.0), end) < 0.05, "it doesn't turn");
      assert.ok(respawn.alive && !respawn.active);
    });

    // A bot driving an AAT, 25 m behind him.
    await check("vehicle killer", async () => {
      await page.evaluate(() => window.__run(30));
      const ready0 = await ready(battle);
      const setup = await world.evaluate(() => {
        const b = self.__bf, dc = self.__dc;
        // An AAT with an enemy at the controls: the AI's own driver, else
        // one put there.
        const enemy = (unit) => unit && b.units.relation(unit.team, b.playerTeam) < 0;
        const aats = b.vehicles.vehicles.filter((v) => v.alive && v.name.includes("aat"));
        let aat = aats.find((v) => enemy(v.seats[0].lease?.unit));
        let bot = aat?.seats[0].lease.unit ?? null;
        let boarded = Boolean(bot);
        if (!aat) {
          aat = aats.find((v) => v.seatFree(0));
          bot = dc.enemy();
          if (!aat || !bot) return { missing: { aat: Boolean(aat), bot: Boolean(bot) } };
          boarded = b.vehicles.board(bot, aat, 0);
        }
        // He stands in front of it, facing away.
        const yaw = aat.yaw;
        const ahead = { x: aat.position.x + Math.sin(yaw) * 25, z: aat.position.z + Math.cos(yaw) * 25 };
        dc.putPlayer(ahead.x, aat.position.y + 1, ahead.z, yaw);
        dc.aat = bot;
        return { boarded };
      });
      setup.ready = ready0;
      assert.equal(setup.missing, undefined, JSON.stringify(setup.missing));
      await page.evaluate(() => window.__run(30));
      // Textures for the new spot, then the shot.
      await settle(page);
      Object.assign(setup, await world.evaluate(() => {
        const b = self.__bf;
        const bot = self.__dc.aat;
        const alive = b.playerUnit.alive;
        b.combat.damage(Infinity, Infinity, null, bot);
        return { alive: alive && b.playerUnit.alive === false, active: b.deathView.active, killer: b.deathKiller?.unit === bot, seated: Boolean(bot.seat?.vehicle) };
      }));
      const samples = await sequence(battle, "vehicle");
      report("vehicle killer", samples);
      const respawn = await back(battle);
      assert.deepEqual(errors, []);
      assert.ok(setup.ready && setup.alive && setup.boarded && setup.seated && setup.active && setup.killer, JSON.stringify(setup));
      // It drives on: the camera trails it a little until its fraction is 1.
      const framed = at(samples, 2.0);
      assert.ok(framed.killerOff < 10, `the AAT framed by 2 s (${framed.killerOff.toFixed(2)})`);
      for (const s of samples.filter((s) => s.t >= 3.6 && s.active)) assert.ok(s.killerOff < 1, `locked on it at ${s.t.toFixed(2)} s (${s.killerOff.toFixed(2)})`);
      assert.ok(respawn.alive && !respawn.active);
    });

    // Killed with the tank he drives: an enemy soldier destroys it.
    await check("in a vehicle", async () => {
      await page.evaluate(() => window.__run(30));
      const ready0 = await ready(battle);
      const found = await world.evaluate(() => {
        const b = self.__bf, dc = self.__dc;
        // The nearest hull he can take the controls of (a bot there makes
        // way), a tank first.
        const near = (a, c) => a.position.distanceTo(b.soldier.position) - c.position.distanceTo(b.soldier.position);
        const open = b.vehicles.vehicles.filter((v) => v.alive && !v.flying && (v.seatFree(0) || v.crewed(0))).sort(near);
        const tank = open.find((v) => !v.name.includes("aat")) ?? open[0];
        const killer = dc.enemy() ?? [...b.units.living()].find((u) => !u.human && u.soldier && b.units.relation(u.team, b.playerTeam) < 0 && u.seat?.vehicle !== tank);
        if (!tank || !killer) return { missing: { tank: Boolean(tank), killer: Boolean(killer) } };
        if (killer.seat) b.vehicles.unboard(killer, { placement: true });
        dc.putPlayer(tank.position.x + 3, tank.position.y + 1, tank.position.z, 0);
        // Whoever destroys it stands 40 m off, out of the blast.
        dc.place(killer, tank.position.x - 40, tank.position.y + 1, tank.position.z + 6, Math.PI / 2);
        dc.tank = tank; dc.destroyer = killer;
        return {};
      });
      assert.equal(found.missing, undefined, JSON.stringify(found.missing));
      await page.evaluate(() => window.__run(2));
      const entered = await world.evaluate(() => self.__bf.driver.enter(self.__dc.tank));
      await page.evaluate(() => window.__run(30));
      const r = await world.evaluate(() => {
        const b = self.__bf, { tank, destroyer: killer } = self.__dc;
        const view = b.simulationCamera.position.clone();
        const alive = b.playerUnit.alive && b.driver.active;
        tank.damage(1e9, killer);
        return {
          alive, dead: b.combat.dead, credited: b.combat.lastAttacker === killer, active: b.deathView.active,
          killer: b.deathKiller?.unit === killer, fromHull: b.deathView.position.distanceTo(view) < 1e-9, driving: b.driver.active,
        };
      });
      Object.assign(r, { entered, alive: ready0 && r.alive });
      await page.evaluate(() => window.__run(150));
      Object.assign(r, await world.evaluate(() => {
        const b = self.__bf, dc = self.__dc;
        const eyes = dc.killer();
        // If he died meanwhile, it looks down on the body again.
        return { framed: eyes ? dc.off(b.simulationCamera, eyes) : null, body: dc.off(b.simulationCamera, dc.body()) };
      }));
      const respawn = await back(battle);
      console.log(`in a vehicle: ${JSON.stringify(r)}`);
      assert.deepEqual(errors, []);
      assert.ok(r.alive && r.entered && r.dead && !r.driving, JSON.stringify(r));
      assert.ok(r.credited && r.killer, "credited to whoever destroyed the tank, and turned to");
      assert.ok(r.active && r.fromHull, "from the tank's view");
      assert.ok(r.framed === null ? r.body < 3 : r.framed < 3, `framed by 2.5 s (${r.framed}, body ${r.body})`);
      assert.ok(respawn.alive && !respawn.active);
    });

    // The drawn camera at 60 and 144 frames a second and at uneven frames:
    // the same smooth path, on the killer the world named by the end.
    await check("drawn at any frame rate", async () => {
      const peaks = [];
      for (const [name, pattern] of [["60 Hz", [1 / 60]], ["144 Hz", [1 / 144]], ["6 ms / 20 ms", [0.006, 0.02]]]) {
        const { id, named, samples } = await drawnDeath(battle, pattern);
        const m = motion(samples);
        const end = samples.at(-1);
        console.log(`drawn ${name}: ${show(m)}, killer ${end.killerOff?.toFixed(1)} deg off at ${end.t.toFixed(2)} s`);
        assert.ok(m.reversals === 0 && m.spikes === 0 && m.accel < 60 && m.spin < 900, `${name}: ${show(m)}`);
        assert.ok(samples.every((s) => s.known), `${name}: the killer known throughout`);
        assert.equal(named, id, `${name}: the page knows the killer the world named`);
        assert.ok(end.killerOff < 6, `${name}: on him at the end (${end.killerOff?.toFixed(1)})`);
        peaks.push(m.accel);
      }
      assert.ok(Math.max(...peaks) / Math.min(...peaks) < 1.5, `the same at every frame rate (${peaks.map((p) => p.toFixed(1))})`);
      assert.deepEqual(errors, []);
    });

    // Classic draws the world camera's framing: down on the body, then over
    // it at him.
    await check("classic drawn", async () => {
      await page.evaluate(() => window.__bf.settings.set("deathCamera", "classic"));
      await ready(battle);
      const id = await world.evaluate(() => {
        const b = self.__bf, dc = self.__dc;
        const p = b.soldier.position;
        const bot = dc.enemy();
        const yaw = b.soldier.aimYaw + (2 * Math.PI) / 3;
        dc.place(bot, p.x + Math.sin(yaw) * 20, p.y, p.z + Math.cos(yaw) * 20, yaw + Math.PI);
        bot.combat.health = 1e9;
        return bot.id;
      });
      await page.evaluate(() => window.__run(1));
      await world.evaluate((id) => self.__bf.combat.damage(Infinity, Infinity, null, self.__bf.units.byId(id)), id);
      const first = await start(world);
      const frames = await page.evaluate(async () => {
        const out = {};
        for (let tick = 1; tick <= 210; tick++) {
          await window.__bf.lockstep(1 / 60, false);
          if (tick === 36 || tick === 150) out[tick] = window.__drawn();
        }
        return out;
      });
      const ticks = await recorded(world);
      const r = Object.fromEntries(Object.entries(frames).map(([tick, frame]) => [tick, merge(ticks.get(frame.tick), frame, first.tick)]));
      await page.evaluate(() => window.__bf.settings.set("deathCamera", "enhanced"));
      await back(battle);
      console.log(`classic drawn: 0.6 s body ${r[36].drawnBodyOff.toFixed(1)} (world ${r[36].bodyOff.toFixed(1)}), 2.5 s killer ${r[150].drawnKillerOff?.toFixed(1)} (world ${r[150].killerOff?.toFixed(1)})`);
      assert.ok(r[36].drawnBodyOff < 4, `down on the body at 0.6 s (${r[36].drawnBodyOff.toFixed(1)})`);
      assert.ok(r[150].drawnKillerOff < 5, `on him at 2.5 s (${r[150].drawnKillerOff?.toFixed(1)})`);
    });
    await battle.close();
  }

  // ------------------------------------------------------------ the droideka: none
  {
    const battle = await open(`map=${MAP}&unit=cis_inf_droideka&ai=off`);
    const { page, world, errors } = battle;
    await check("droideka", async () => {
      const drawn = await page.evaluate(async () => {
        const b = window.__bf;
        await window.__run(60);
        b.setPerspective(true);
        await window.__run(5);
        b.spawnPort.suicide();
        await window.__run(90);
        const eye = new b.THREE.Vector3(0, 1.8, 0).add(b.soldier.position);
        return { drawnDistance: b.camera.position.distanceTo(eye), bodyDrawn: b.actor.root.children.some((child) => child.layers.test(b.camera.layers)) };
      });
      const r = await world.evaluate(() => {
        const b = self.__bf;
        const eye = new b.THREE.Vector3(0, 1.8, 0).add(b.soldier.position);
        return { dead: b.combat.dead, active: b.deathView.active, distance: b.simulationCamera.position.distanceTo(eye) };
      });
      Object.assign(r, drawn);
      await page.evaluate(() => window.__bf.setPerspective(false));
      console.log(`droideka: ${JSON.stringify(r)}`);
      assert.deepEqual(errors, []);
      assert.ok(r.dead && !r.active, "no death camera for a droideka");
      assert.ok(r.distance > 2 && r.drawnDistance > 2, "its chase camera, out of its eyes");
      assert.ok(r.bodyDrawn, "the wreck is drawn");
    });
    await battle.close();
  }

  // ------------------------------------------------------------ a jet trooper in the air
  {
    const battle = await open(`map=${MAP}&unit=rep_inf_jet_trooper&ai=off`);
    const { page, world, errors } = battle;
    await check("jet trooper", async () => {
      await page.evaluate(async () => {
        const b = window.__bf;
        await window.__run(60);
        b.pending.jump = true;
        await window.__run(10);
        b.pending.jump = true;
        b.keys.add("KeyW");
        await window.__run(45);
        b.keys.delete("KeyW");
      });
      const r = await world.evaluate(() => {
        const b = self.__bf, T = b.THREE;
        const hit = b.collision.cast(b.soldier.position.clone(), new T.Vector3(0, -1, 0), 100, { point: new T.Vector3(), normal: new T.Vector3() });
        return { air: b.soldier.air, height: hit ? hit.distance : null, at: b.soldier.position.y };
      });
      await page.evaluate(async () => {
        window.__bf.spawnPort.suicide();
        await window.__run(1);
      });
      const first = await world.evaluate(() => self.__bf.deathView.body.y);
      await page.evaluate(() => window.__run(90));
      Object.assign(r, await world.evaluate((first) => {
        const b = self.__bf, dc = self.__dc;
        return { active: b.deathView.active, fell: first - b.deathView.body.y, pitch: dc.pitch(b.simulationCamera), bodyOff: dc.off(b.simulationCamera, dc.body()) };
      }, first));
      console.log(`jet trooper: ${JSON.stringify(r)}`);
      assert.deepEqual(errors, []);
      assert.equal(r.air, "jet", "flying when killed");
      assert.ok(r.height > 1, `${r.height} m up`);
      assert.ok(r.active && r.fell > 1, `it follows the body down (${r.fell.toFixed(2)} m)`);
      assert.ok(r.bodyOff < 3, "looking at the body where it lies");
    });
    await battle.close();
  }
} catch (error) {
  // A page that didn't load ends the run; what ran before still reports.
  failures.push(error.stack);
} finally {
  await browser.close();
}

if (failures.length) {
  console.error(failures.join("\n\n"));
  process.exit(1);
}
console.log("death camera: pass");
