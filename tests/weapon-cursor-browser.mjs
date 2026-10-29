// The arc trooper's missile lock as drawn: the world (in its worker) locks
// his launcher onto a still target ahead of him, and the page's lock marker
// sits over that target in third and first person; his pistol has none.
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import { launchBrowser, openBattle } from './battle-page.mjs';
const out = process.env.CURSOR_ARTIFACTS || '/tmp/bf-cursor-review';
await fs.mkdir(out, { recursive: true });
const browser = await launchBrowser();
try {
  const battle = await openBattle(browser, { base: process.env.RUNTIME_AUDIT_URL || 'http://127.0.0.1:18449', query: 'map=nab2&side=rep&unit=rep_inf_arc_trooper&ai=off&seed=1',
    settings: { waterQuality: 'low', shadows: 'off', antialiasing: 'off', environmentEffects: 'classic' } });
  const { page, world, errors } = battle;
  console.log('assets ready');
  // Alive on the plaza; the page shows him there from the next tick.
  await world.evaluate(() => {
    const b = self.__bf;
    b.combat.dead = false; b.combat.health = b.combat.maxHealth; b.playerUnit.alive = true;
    b.soldier.spawn(-51.76, -50.04, -27.93, 0); b.combat.aim();
  });
  await battle.frame(1 / 60);
  await page.evaluate(() => { __bf.controls.locked = true; __bf.presentation.reset(); });
  await battle.frames(60);
  // A clear line ahead of him and a still enemy on it, in the world.
  const target = await world.evaluate(() => {
    const b = self.__bf, T = b.THREE;
    const weapon = b.combat.arsenal.current(0);
    if (weapon.def.base !== 'launcher') throw Error(`wrong unit: ${weapon.name}`);
    const hit = { point: new T.Vector3(), normal: new T.Vector3() };
    let ray = null;
    search: for (const distance of [20, 12, 6]) for (const pitch of [0, .4, .8]) for (let i = 0; i < 32; i++) {
      const a = i * Math.PI / 16, dir = new T.Vector3(Math.sin(a) * Math.cos(pitch), Math.sin(pitch), Math.cos(a) * Math.cos(pitch));
      if (!b.collision.cast(b.combat.origin, dir, distance, hit)) { ray = { yaw: a, pitch, distance }; break search; }
    }
    if (!ray) throw Error(`no clear test ray: ${JSON.stringify({ origin: b.combat.origin.toArray(), feet: b.soldier.position.toArray(), dead: b.combat.dead })}`);
    b.soldier.aimYaw = ray.yaw; b.soldier.aimPitch = ray.pitch; b.combat.aim();
    const position = b.combat.origin.clone().addScaledVector(b.combat.aimDir, ray.distance * .85);
    const unit = { generation: 1, alive: true, health: 100, maxHealth: 100, team: b.playerTeam === 1 ? 2 : 1,
      kind: 6, position, velocity: new T.Vector3(), sphere: { center: position, radius: .3 }, classDef: {},
      aimPoint(out) { return out.copy(position); } };
    b.units.add(unit);
    return { id: unit.id, team: unit.team, position: position.toArray() };
  });
  // The page's copy of it under the world's id, as its copy of any unit: the
  // world's packets keep it.
  await page.evaluate(({ id, team, position }) => {
    const b = __bf, T = b.THREE, at = new T.Vector3(...position);
    const unit = { generation: 1, alive: true, health: 100, maxHealth: 100, team,
      kind: 6, position: at, velocity: new T.Vector3(), sphere: { center: at, radius: .3 }, classDef: {},
      aimPoint(out) { return out.copy(this.position); } };
    b.units.add(unit);
    b.client.replica.assignUnitId(b.units, unit, id);
    window.__cursorTarget = unit;
  }, target);
  const samples = [];
  for (const first of [false, true]) {
    await page.evaluate((first) => __bf.setPerspective(first), first);
    await world.evaluate(() => self.__bf.combat.arsenal.current(0).clearLock());
    await battle.frames(70);
    samples.push(await page.evaluate((first) => {
      const b = __bf, weapon = b.combat.arsenal.current(0);
      const expected = window.__cursorTarget.position.clone().project(b.camera), marker = b.hud.lockMarker;
      return { first, complete: weapon.lockComplete, id: weapon.lockTargetId,
        progress: marker?.progress, error: marker ? Math.hypot(marker.x - (expected.x + 1) / 2, marker.y - (1 - expected.y) / 2) : Infinity };
    }, first));
  }
  const capture = await page.evaluate(async () => {
    const b = __bf;
    b.setPerspective(false);
    for (let i = 0; i < 30; i++) await b.lockstep(1 / 60, false);
    await new Promise(r => setTimeout(r, 300)); await b.lockstep(0, true);
    return b.renderer.domElement.toDataURL('image/png');
  });
  // The pistol: no lock, no marker.
  await world.evaluate(() => self.__bf.combat.arsenal.next(0));
  await battle.frames(70);
  const pistol = await page.evaluate(() => {
    const b = __bf, weapon = b.combat.arsenal.current(0);
    return { base: weapon.def.base, marker: b.hud.lockMarker, target: weapon.lockTargetId };
  });
  const result = { samples, pistol, target: target.id };
  for (const row of result.samples) {
    assert.equal(row.complete, true, JSON.stringify(row)); assert.equal(row.progress, 1);
    assert.equal(row.id, target.id, JSON.stringify(row));
    assert.ok(row.error < 1e-8, JSON.stringify(row));
  }
  assert.equal(result.pistol.marker, null); assert.equal(result.pistol.target, -1);
  assert.deepEqual(errors, []);
  await fs.writeFile(`${out}/arc-chase.png`, Buffer.from(capture.split(',')[1], 'base64'));
  await fs.writeFile(`${out}/report.json`, JSON.stringify(result, null, 2));
  console.log(JSON.stringify(result));
} finally { await browser.close(); }
