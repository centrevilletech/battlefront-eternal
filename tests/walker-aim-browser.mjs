// Walker guns as a player fires them (docs/vehicles.md), on Hoth: for the
// AT-AT and the AT-ST, each seat's weapons, which nodes they fire from,
// where the crosshair's point is, and where each round goes; plus a side
// view of the walker on the snow. Screenshots in OUT.
import fs from 'node:fs';
import { launchBrowser, openBattle } from './battle-page.mjs';
const BASE = process.env.BASE || 'http://127.0.0.1:18449';
const OUT = process.env.OUT || '/tmp/bfe-walker-shots';
fs.mkdirSync(OUT, { recursive: true });
const browser = await launchBrowser();
const battle = await openBattle(browser, { base: BASE, query: 'map=hot1&side=imp&unit=imp_inf_storm_troopersnow&ai=off&seed=1', viewport: { width: 960, height: 540 } });
const { page, world } = battle;
await battle.frames(5);
for (const [name, seats] of [['imp_walk_atat', [0, 1]], ['imp_walk_atst_snow', [0, 1]]]) {
  for (const seat of seats) {
    const index = await world.evaluate(([name, seat]) => {
      const b = self.__bf, unit = self.__server.actor.unit;
      if (unit.seat) b.vehicles.unboard(unit, { force: true });
      const v = b.vehicles.vehicles.find((v) => v.name === name && v.alive);
      unit.soldier.position.copy(v.position).add(new b.THREE.Vector3(3, 0, 0));
      return b.vehicles.board(unit, v, seat) ? b.vehicles.vehicles.indexOf(v) : -1;
    }, [name, seat]);
    await battle.frames(30, 1 / 60, true);
    const info = await page.evaluate(([index, seat]) => {
      const b = __bf, v = b.vehicles.vehicles[index];
      const nameOf = (bone) => bone ? Object.keys(v.byName).find((k) => v.byName[k] === bone) || bone.name || '(marker)' : null;
      return { seatIndex: v.seatIndex, weapons: v.seats[seat].weapons.map((w) => ({
        weapon: w.def.class, aimers: w.aimers.map((a) => ({ node: a.node, yaw: a.yawLimits.map((x) => Math.round(x * 57.3)), pitch: a.pitchLimits.map((x) => Math.round(x * 57.3)), fire: a.FirePointName })),
        muzzles: w.muzzles.map((m) => nameOf(m.fireNode?.parent && !v.byName[m.fireNode.name?.toLowerCase()] ? m.fireNode.parent : m.fireNode)) })) };
    }, [index, seat]);
    console.log(name, 'seat', seat, JSON.stringify(info));
    const view = await page.evaluate(([index, seat]) => {
      const b = __bf, v = b.vehicles.vehicles[index], d = new b.THREE.Vector3();
      b.camera.getWorldDirection(d);
      const r = b.hud.reticle;
      return { reticle: r ? { x: +(r.x ?? .5).toFixed(3), y: +r.y.toFixed(3), weapon: r.weapon?.def?.class, texture: r.weapon?.def?.ReticuleTexture } : null,
        seatPitch: +(v.seats[seat].pitch * 57.3).toFixed(1), cameraPitch: +(Math.asin(d.y) * 57.3).toFixed(1), firstPerson: b.firstPerson ?? null };
    }, [index, seat]);
    console.log('  view', JSON.stringify(view));
    // Fire for two seconds; the world's rounds: where they start and go
    // versus the point under the crosshair.
    await world.evaluate((index) => {
      const b = self.__bf, v = b.vehicles.vehicles[index];
      self.__shots = [];
      v.__emit ||= v.emitShot;
      v.emitShot = function (shot, i) {
        self.__shots.push({ seat: i, from: v.muzzle.toArray().map((n) => +n.toFixed(2)), dir: v.direction.toArray().map((n) => +n.toFixed(3)),
          target: v.seats[i].target?.toArray().map((n) => +n.toFixed(1)) ?? null, node: v.shotNode?.name, onTarget: null });
        return v.__emit.apply(v, arguments);
      };
    }, index);
    await page.evaluate(async () => { const b = __bf; b.controls.hold('Mouse0'); b.controls.press('Mouse0'); });
    await battle.frames(120, 1 / 60, true);
    await page.evaluate(() => __bf.controls.down.delete('Mouse0'));
    const shots = await world.evaluate(() => self.__shots.slice(0, 6));
    // How far each round's line passes from the crosshair's point.
    for (const s of shots) if (s.target) {
      const d = s.target.map((t, k) => t - s.from[k]), along = d[0] * s.dir[0] + d[1] * s.dir[1] + d[2] * s.dir[2];
      const miss = Math.hypot(...d.map((x, k) => x - s.dir[k] * along));
      s.miss = +miss.toFixed(2); s.range = +Math.hypot(...d).toFixed(1);
    }
    console.log('  world shots', JSON.stringify(shots.map(({ node, from, miss, range }) => ({ node, from, miss, range }))));
    await page.screenshot({ path: `${OUT}/${name}-seat${seat}.png` });
  }
}
// The AT-AT walking, seen from the side, its feet on the snow.
await world.evaluate(() => { const b = self.__bf, unit = self.__server.actor.unit; if (unit.seat) b.vehicles.unboard(unit, { force: true }); });
await battle.frames(10);
for (const name of ['imp_walk_atat', 'imp_walk_atst_snow']) {
  const index = await world.evaluate((name) => {
    const b = self.__bf, unit = self.__server.actor.unit, v = b.vehicles.vehicles.find((v) => v.name === name && v.alive);
    unit.soldier.position.copy(v.position).add(new b.THREE.Vector3(3, 0, 0));
    return b.vehicles.board(unit, v, 0) ? b.vehicles.vehicles.indexOf(v) : -1;
  }, name);
  await battle.frames(10);
  await page.evaluate(() => __bf.keys.add('KeyW'));
  for (let f = 0; f < 4; f++) {
    await battle.frames(45);
    const feet = await page.evaluate((index) => {
      const b = __bf, v = b.vehicles.vehicles[index], p = new b.THREE.Vector3();
      const rows = (v.def.fx?.feet || []).map((foot) => {
        const bone = v.byName[foot.bone]; if (!bone) return null;
        bone.getWorldPosition(p);
        const hit = b.collision.cast(new b.THREE.Vector3(p.x, p.y + 20, p.z), new b.THREE.Vector3(0, -1, 0), 60, { point: new b.THREE.Vector3(), normal: new b.THREE.Vector3() }, v.body);
        const ground = hit ? hit.point.y : null;
        return `${foot.bone}:${(p.y - (ground ?? NaN)).toFixed(2)}`;
      });
      const yaw = v.root.rotation.y + Math.PI / 2, at = v.root.position, far = v.size.y * 2.6;
      b.camera.position.set(at.x + Math.sin(yaw) * far, at.y + v.size.y * .5, at.z + Math.cos(yaw) * far);
      b.camera.lookAt(at.x, at.y + v.size.y * .45, at.z); b.camera.updateMatrixWorld(); b.graphics.render(0);
      return { rows, rootY: `${at.x.toFixed(1)},${at.y.toFixed(2)},${at.z.toFixed(1)}`, image: b.renderer.domElement.toDataURL() };
    }, index);
    console.log(name, 'walk', f, feet.rootY, feet.rows.join(' '));
    fs.writeFileSync(`${OUT}/${name}-walk${f}.png`, Buffer.from(feet.image.split(',')[1], 'base64'));
  }
  await page.evaluate(() => __bf.keys.clear());
  await world.evaluate(() => { const b = self.__bf, unit = self.__server.actor.unit; if (unit.seat) b.vehicles.unboard(unit, { force: true }); });
  await battle.frames(10);
}
console.log('errors', battle.errors.slice(0, 5));
await browser.close();
