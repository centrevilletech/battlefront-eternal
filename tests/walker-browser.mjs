// Walkers as the page draws them (src/vehicles.js Vehicle.present,
// src/vehicle-feet.js, src/vehicle-effects.js), on Hoth:
// - his own AT-ST and AT-AT driven in third person: the legs walk and the
//   feet stomp (the page's StompEffect at the foot, level with the hull's
//   root), stepping the world worker frame by frame (tests/battle-page.mjs);
// - his own AT-ST on the page's own clock, his hull predicted ahead of the
//   world: the predicted legs walk and stomp too;
// - a bot's AT-ST (Classic AI, seed 1), drawn from its records: the same.
// VEHICLE_TEST_URL is the Vite server (Hoth extracted). Screenshots go to
// /tmp/bf-walker-*.png.
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import { launchBrowser, openBattle } from './battle-page.mjs';

const base = process.env.VEHICLE_TEST_URL || 'http://127.0.0.1:18449';
const query = (ai) => `map=hot1&side=imp&unit=imp_inf_storm_troopersnow&ai=${ai}&seed=1&shadows=off&loading=0`;
const shot = (image, name) => fs.writeFile(`/tmp/bf-walker-${name}.png`, Buffer.from(image.split(',')[1], 'base64'));

// On the page: per frame, each foot's drawn place in the hull's frame and
// every stomp effect the page plays for that hull. `step` advances a frame.
const watch = async (page, index, frames, step) => page.evaluate(async ([index, frames, step]) => {
  const b = __bf, v = b.vehicles.vehicles[index], V = b.THREE.Vector3, point = new V(), inverse = new b.THREE.Matrix4();
  const feet = v.def.fx.feet.map((foot) => v.byName[foot.bone]);
  const stomps = [], poses = new Set(), play = b.effects.play;
  b.effects.play = function (name, position, normal, velocity, quaternion) {
    if (name === v.def.fx.stomp?.effect && Math.hypot(position.x - v.root.position.x, position.z - v.root.position.z) < v.radius * 2) {
      stomps.push({ y: position.y - v.root.matrixWorld.elements[13], up: quaternion ? new V(0, 1, 0).applyQuaternion(quaternion).y : null });
    }
    return play.apply(this, arguments);
  };
  const start = v.root.position.clone();
  try {
    for (let i = 0; i < frames; i++) {
      if (step === 'lockstep') await b.lockstep(1 / 60, false);
      else { await new Promise((r) => setTimeout(r, 16)); b.frame(1 / 60, false); }
      inverse.copy(v.root.matrixWorld).invert();
      poses.add(feet.map((bone) => bone.getWorldPosition(point).applyMatrix4(inverse).toArray().map((n) => n.toFixed(2)).join()).join('|'));
    }
  } finally { b.effects.play = play; }
  const yaw = v.root.rotation.y + Math.PI / 2, at = v.root.position, far = v.size.y * 1.6;
  b.camera.position.set(at.x + Math.sin(yaw) * far, at.y + far * .3, at.z + Math.cos(yaw) * far);
  b.camera.lookAt(at.x, at.y + v.size.y * .4, at.z); b.camera.updateMatrixWorld(); b.graphics.render(0);
  return { name: v.name, poses: poses.size, stomps, moved: start.distanceTo(v.root.position), networkPose: Boolean(v.networkPose),
    predicted: Boolean(b.client.vehiclePrediction), image: b.renderer.domElement.toDataURL() };
}, [index, frames, step]);

// Into the walker as the page's player (the world's network human).
const board = (world, name) => world.evaluate((name) => {
  const b = self.__bf, unit = self.__server.actor.unit, v = b.vehicles.vehicles.find((v) => v.name === name && v.alive && !v.seats[0].lease);
  unit.soldier.position.copy(v.position).add(new b.THREE.Vector3(3, 0, 0));
  return b.vehicles.board(unit, v, 0) ? b.vehicles.vehicles.indexOf(v) : -1;
}, name);

const report = (label, r) => console.log(JSON.stringify({ label, name: r.name, poses: r.poses, stomps: r.stomps.length, moved: +r.moved.toFixed(1), networkPose: r.networkPose, predicted: r.predicted }));
const checkStomps = (r, label) => {
  assert.ok(r.stomps.length >= 2, `${label}: stomps ${r.stomps.length}`);
  for (const s of r.stomps) {
    assert.ok(Math.abs(s.y) < 1e-3, `${label}: a stomp ${s.y} m off the root's level`);
    if (s.up !== null) assert.ok(s.up > .9, `${label}: a stomp turned ${s.up}`);
  }
};

const browser = await launchBrowser();
try {
  {
    const battle = await openBattle(browser, { base, query: query('off'), viewport: { width: 960, height: 600 } });
    const { page, world, errors } = battle;
    await page.evaluate(() => __bf.setPerspective(false));
    await battle.frames(5);
    for (const [name, frames] of [['imp_walk_atst_snow', 240], ['imp_walk_atat', 420]]) {
      const index = await board(world, name);
      assert.ok(index >= 0, `boards a ${name}`);
      await battle.frames(10);
      assert.equal(await page.evaluate((index) => __bf.driver.vehicle === __bf.vehicles.vehicles[index], index), true, `the page seats him in the ${name}`);
      await page.evaluate(() => __bf.keys.add('KeyW'));
      const r = await watch(page, index, frames, 'lockstep');
      await page.evaluate(() => __bf.keys.clear());
      report(`own ${name}, lockstep`, r);
      assert.equal(r.networkPose, false, 'his own hull has no record pose');
      assert.ok(r.moved > 3, `${name} moved ${r.moved}`);
      assert.ok(r.poses > frames * .6, `${name}: the legs walk (${r.poses} poses in ${frames} frames)`);
      checkStomps(r, `own ${name}`);
      await shot(r.image, `own-${name}`);
      await world.evaluate(() => { const b = self.__bf; b.vehicles.unboard(self.__server.actor.unit, { force: true }); });
      await battle.frames(10);
    }
    assert.deepEqual(errors, []);
    await battle.close();
  }
  {
    // The page's own clock (no lockstep): his hull predicted.
    const battle = await openBattle(browser, { base, query: query('off'), viewport: { width: 640, height: 400 } });
    const { page, world, errors } = battle;
    const run = (n) => page.evaluate(async (n) => { const b = __bf; b.resume(); for (let i = 0; i < n; i++) { await new Promise((r) => setTimeout(r, 16)); b.frame(1 / 60, false); } }, n);
    await page.evaluate(() => __bf.setPerspective(false));
    await run(20);
    const index = await board(world, 'imp_walk_atst_snow');
    assert.ok(index >= 0);
    await run(30);
    await page.evaluate(() => __bf.keys.add('KeyW'));
    const r = await watch(page, index, 240, 'clock');
    report('own imp_walk_atst_snow, predicted', r);
    assert.equal(r.predicted, true, 'his hull is predicted');
    assert.ok(r.poses > 120, `the predicted legs walk (${r.poses})`);
    checkStomps(r, 'predicted');
    assert.deepEqual(errors, []);
    await battle.close();
  }
  {
    // A bot drives an AT-ST: the page draws it from its records.
    const battle = await openBattle(browser, { base, query: query('classic'), viewport: { width: 960, height: 600 } });
    const { page, world, errors } = battle;
    let index = -1;
    for (let second = 0; second < 90 && index < 0; second += 5) {
      await battle.frames(300);
      index = await world.evaluate(() => self.__bf.vehicles.vehicles.findIndex((v) => v.name === 'imp_walk_atst_snow' && v.alive && v.seats[0].lease && !v.seats[0].lease.human && v.velocity.length() > 3));
    }
    assert.ok(index >= 0, 'a bot walks an AT-ST within 90 s');
    const r = await watch(page, index, 480, 'lockstep');
    report('bot imp_walk_atst_snow', r);
    assert.equal(r.networkPose, true);
    assert.ok(r.poses > 60, `the bot's AT-ST walks (${r.poses})`);
    checkStomps(r, 'bot');
    await shot(r.image, 'bot-imp_walk_atst_snow');
    assert.deepEqual(errors, []);
    await battle.close();
  }
} finally { await browser.close(); }
