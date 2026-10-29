// Requires Vite plus extracted Theed (nab2). Every seat of both Theed tanks,
// in third and first person: the rider shows only where the seat has one, the
// hull leaves a first-person view, the view's field of view is the seat's,
// and a round fired at a wall 20 m or 100 m down the reticle's ray leaves a
// real fire point and passes through the reticle's own hit. The world (in
// its worker, tests/battle-page.mjs) seats him and fires the round; the page
// draws the seat's view and its reticle. The wall stands in both: the
// world's round hits it, the page's reticle ray meets it. PLAYWRIGHT_MODULE
// can point at an external Playwright install.
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import { launchBrowser, openBattle } from './battle-page.mjs';
const base = process.env.VEHICLE_TEST_URL || 'http://127.0.0.1:18449';
const out = process.env.VEHICLE_TEST_ARTIFACTS || '/tmp/bf-vehicle-seats';
await fs.mkdir(out, { recursive: true });

// The test wall in this thread's world (globalThis.__bf: the page's or the
// worker's): a slab square to the reticle's ray, placed by `matrix` (a
// Matrix4's 16 numbers) when given, in the collision world when `enabled`.
const wall = ({ matrix = null, enabled }) => {
  const b = globalThis.__bf;
  if (!globalThis.__seatWall) {
    const x = 40, h = 40, z = .25, p = [[-x,-h,-z],[x,-h,-z],[x,h,-z],[-x,h,-z],[-x,-h,z],[x,-h,z],[x,h,z],[-x,h,z]], f = [[0,1,2],[0,2,3],[4,6,5],[4,7,6],[0,4,5],[0,5,1],[3,2,6],[3,6,7],[0,3,7],[0,7,4],[1,5,6],[1,6,2]];
    const shape = b.collision.createBodyShape(new Float32Array(p.flat()), new Uint32Array(f.flat()), new Uint32Array(f.length).map((_, k) => k));
    globalThis.__seatWallMatrix = new b.THREE.Matrix4();
    globalThis.__seatWall = b.collision.addMovingBody(shape, globalThis.__seatWallMatrix, null);
  }
  if (matrix) b.collision.moveBody(globalThis.__seatWall, globalThis.__seatWallMatrix.fromArray(matrix));
  globalThis.__seatWall.enabled = enabled;
};
// The distance from a point to a round's line.
const missOf = (point, start, dir) => {
  const rel = point.map((v, i) => v - start[i]), along = rel.reduce((sum, v, i) => sum + v * dir[i], 0);
  return Math.hypot(...rel.map((v, i) => v - along * dir[i]));
};

const browser = await launchBrowser();
try {
  const battle = await openBattle(browser, { base, query: 'map=nab2&side=rep&unit=rep_inf_clone_trooper&shadows=off', viewport: { width: 800, height: 450 }, settings: null });
  const { page, world, errors } = battle;
  page.on('console', (m) => { if (m.type() !== 'error' && /GL_INVALID|WebGL.*error/.test(m.text())) errors.push(m.text()); });
  await page.evaluate(() => { __bf.settings.set('waterQuality', 'low'); __bf.settings.set('treeDetail', 'original'); });
  const missing = [];
  for (const name of ['rep_hover_fightertank', 'cis_hover_aat']) {
    const seats = await world.evaluate((name) => self.__bf.vehicles.vehicles.find((v) => v.name === name && v.alive)?.seats.length ?? 0, name);
    if (!seats) { console.log(JSON.stringify({ name, missing: 'no such hull alive on this map' })); missing.push(name); continue; }
    for (let seat = 0; seat < seats; seat++) for (const first of [false, true]) {
      const rows = [];
      for (const D of [20, 100]) {
        // At the seat in the world: the hull turned to its most open
        // heading (for the 100 m wall) when he boards.
        await world.evaluate(({ name, seat }) => {
          const b = self.__bf, V = b.THREE.Vector3;
          const v = b.vehicles.vehicles.find((x) => x.name === name && x.alive);
          if (b.driver.vehicle !== v) {
            b.driver.exit(true); b.combat.reset();
            const probe = { distance: 0, point: new V(), normal: new V() }, from = v.position.clone().add(new V(0, 4, 0));
            let best = v.yaw, room = 0;
            for (let k = 0; k < 36; k++) {
              const yaw = k * Math.PI / 18, d = new V(Math.sin(yaw), .08, Math.cos(yaw)).normalize();
              const hit = b.collision.cast(from, d, 400, probe, v.body);
              if ((hit ? hit.distance : 400) > room) { room = hit ? hit.distance : 400; best = yaw; }
            }
            v.yaw = v.previousYaw = best; v.syncBody();
            b.driver.enter(v);
          }
          for (let k = 0; k < v.seats.length && v.seatIndex !== seat; k++) b.driver.nextSeat(v);
        }, { name, seat });
        await page.evaluate((first) => __bf.setPerspective(first), first);
        // A wall square to the reticle's ray, D metres out; the aim climbs
        // until the wall is the first thing the ray meets.
        let reticle = null;
        for (const pitch of [.03, .12, .22, .32]) {
          await page.evaluate(wall, { enabled: false });
          await world.evaluate(wall, { enabled: false });
          await world.evaluate(({ name, pitch }) => { const v = self.__bf.vehicles.vehicles.find((x) => x.name === name && x.alive); v.lookYaw = v.yaw; v.lookPitch = pitch; }, { name, pitch });
          const matrix = await page.evaluate(async ({ name, D }) => {
            const b = __bf, V = b.THREE.Vector3, Q = b.THREE.Quaternion, aim = new V();
            const v = b.vehicles.vehicles.find((x) => x.name === name && x.alive);
            b.keys.clear();
            for (let i = 0; i < 4; i++) await b.lockstep(1 / 30, false);
            v.viewDirection(aim);
            return new b.THREE.Matrix4().compose(b.camera.position.clone().addScaledVector(aim, D), new Q().setFromUnitVectors(new V(0, 0, 1), aim), new V(1, 1, 1)).toArray();
          }, { name, D });
          await page.evaluate(wall, { matrix, enabled: true });
          await world.evaluate(wall, { matrix, enabled: true });
          reticle = await page.evaluate(async (name) => {
            const b = __bf, V = b.THREE.Vector3, aim = new V(), ray = { distance: 0, point: new V(), normal: new V() };
            const v = b.vehicles.vehicles.find((x) => x.name === name && x.alive);
            for (let i = 0; i < 40; i++) await b.lockstep(1 / 30, false);
            await b.lockstep(1 / 30, true);
            v.viewDirection(aim);
            const hit = b.collision.cast(b.camera.position, aim, 600, ray, v.body);
            return hit?.body === globalThis.__seatWall ? ray.point.toArray() : null;
          }, name);
          if (reticle) break;
        }
        const row = await page.evaluate(({ name, seat, first, D, reticle }) => {
          const b = __bf, v = b.vehicles.vehicles.find((x) => x.name === name && x.alive);
          const seatView = v.seat.view;
          const fov = first ? seatView.firstFov : seatView.thirdFov;
          return { name, seat, first, D, wall: Boolean(reticle),
            rider: b.actor.root.visible, wantsRider: Boolean(seatView.rider),
            hullLayer: v.root.children.find((o) => o.isMesh)?.layers.mask,
            fov: Math.abs(b.camera.fov - 2 * Math.atan(Math.tan(fov / 2) / b.camera.aspect) * 180 / Math.PI) };
        }, { name, seat, first, D, reticle });
        if (reticle) {
          // The round, fired in the world from the page's trigger: where it
          // left from (against the seat's fire points) and its line.
          await world.evaluate((name) => {
            const b = self.__bf, V = b.THREE.Vector3, v = b.vehicles.vehicles.find((x) => x.name === name && x.alive);
            const fireNodes = v.seat.weapons[0].muzzles.map((a) => a.fireNode ?? v.byName[(a.FirePointName || a.node || '').toLowerCase()]).filter(Boolean);
            for (const w of v.seat.weapons) { w.state.heat = 0; w.state.overheated = false; w.state.wait = 0; }
            const shot = self.__seatShot = { old: [new Set(b.ordnance.bolts), new Set(b.ordnance.beams), new Set(b.ordnance.rounds)], from: null, fire: b.ordnance.fire };
            b.ordnance.fire = function (def, origin, ...rest) { shot.from ??= Math.min(...fireNodes.map((n) => n.getWorldPosition(new V()).distanceTo(origin))); return shot.fire.call(this, def, origin, ...rest); };
          }, name);
          await page.evaluate(() => { __bf.pending.firePrimary = true; __bf.keys.add('Mouse0'); });
          let shot = null;
          for (let i = 0; i < 4 && !shot; i++) {
            await battle.frame(1 / 60);
            shot = await world.evaluate(() => {
              const b = self.__bf, { old } = self.__seatShot;
              const round = b.ordnance.bolts.find((x) => !old[0].has(x)) || b.ordnance.beams.find((x) => !old[1].has(x)) || b.ordnance.rounds.find((x) => !old[2].has(x));
              return round ? { start: round.position.toArray(), dir: (round.direction || round.velocity.clone().normalize()).toArray() } : null;
            });
          }
          row.fromFirePoint = await world.evaluate(() => { const b = self.__bf, { fire, from } = self.__seatShot; b.ordnance.fire = fire; return from; });
          await page.evaluate(() => __bf.keys.clear());
          if (shot) {
            row.miss = missOf(reticle, shot.start, shot.dir);
            row.pixels = await page.evaluate((point) => {
              const b = __bf, s = new b.THREE.Vector3(...point).project(b.camera);
              return b.hud.reticle ? Math.hypot((s.x + 1) / 2 * 800 - b.hud.reticle.x * 800, (1 - s.y) / 2 * 450 - b.hud.reticle.y * 450) : null;
            }, reticle);
          }
        }
        console.log(JSON.stringify(row));
        rows.push(row);
        const r = row;
        assert.ok(r.wall, `${name} seat ${seat}: the reticle's ray reaches the test wall`);
        assert.equal(r.rider, r.wantsRider, `${name} seat ${seat}: the rider shows only where the seat has one`);
        assert.equal(r.hullLayer, first ? 2 : 1, `${name} seat ${seat}: the hull leaves the first-person view and keeps its shadow`);
        assert.ok(r.fov < 1e-3, `${name} seat ${seat}: the seat's own field of view`);
        assert.ok(r.miss < .05, `${name} seat ${seat} ${first ? 'first' : 'third'} person at ${D} m: the round passes ${r.miss} m from the reticle's hit`);
        assert.ok(r.fromFirePoint < .01, `${name} seat ${seat}: the round leaves a real fire point`);
        assert.ok(r.pixels !== null && r.pixels < 1, `${name} seat ${seat}: the reticle sits over the hit`);
      }
      const capture = await page.evaluate(async () => { await __bf.lockstep(0, true); return __bf.renderer.domElement.toDataURL(); });
      await fs.writeFile(`${out}/${name}-seat${seat}-${first ? 'first' : 'third'}.png`, Buffer.from(capture.split(',')[1], 'base64'));
    }
  }
  console.log(JSON.stringify({ errors, missing }));
  assert.deepEqual(errors, []);
  assert.deepEqual(missing, [], 'both Theed tanks on the map');
} finally {
  await browser.close();
}
