// Measures where a manned turret's rounds go against the point under its
// reticle. For one turret of each class on the given maps it mans the turret
// through the server's seat path (as tools/turret-probe.mjs does), sets the
// aim (level, or at the lowest or highest pitch its view takes), lets the
// gun settle (2 s), stands a wall square across the aim at each range from
// the drawn camera (in the world's collision and the page's), fires, and
// intersects the server's rounds and the page's reticle ray (from the drawn
// camera through the HUD's reticle anchor) with that wall, in third and
// first person. Prints one JSON line per class, perspective, pitch and
// range: the miss in metres on the wall and in degrees as seen from the
// camera (when the gun was turned to the wall), and, wherever the server's
// aim met something (aimRange from the camera), how far that point lies off
// the reticle's ray (reticleOff) and off the rounds' lines (roundOff), in
// metres. TURRET_PROBE_OUT, when set, also gets a screenshot of each
// third-person shot at the first range.
//
// PLAYWRIGHT_MODULE=... TURRET_TEST_URL=http://127.0.0.1:18494 node tools/turret-aim-probe.mjs kas2,hot1 [10,100] [level,low,high]
// (the last: the aim level, or at the lowest or highest pitch its view takes)
import fs from 'node:fs/promises';
import { launchBrowser, openBattle } from '../tests/battle-page.mjs';

const base = process.env.TURRET_TEST_URL || 'http://127.0.0.1:18493';
const out = process.env.TURRET_PROBE_OUT || null;
const maps = (process.argv[2] || 'kas2,hot1').split(',');
const ranges = (process.argv[3] || '10,100').split(',').map(Number);
// The aim's pitch: level, and the lowest and highest the view takes.
const pitches = (process.argv[4] || 'level,low,high').split(',');
const cw = { side: 'rep', unit: 'rep_inf_clone_trooper' }, gcw = { side: 'imp', unit: 'imp_inf_storm_trooper' }, snow = { side: 'imp', unit: 'imp_inf_storm_troopersnow' };
const eras = { hot1: snow, tat2: gcw, yav1: gcw, end1: gcw, bes2: gcw, bes1: gcw, rhn1: gcw, rhn2: gcw, yav2: gcw };
if (out) await fs.mkdir(out, { recursive: true });

const browser = await launchBrowser();
try {
  for (const map of maps) {
    const { side, unit } = eras[map] || cw;
    const battle = await openBattle(browser, { base, query: `map=${map}&side=${side}&unit=${unit}&shadows=low`, viewport: { width: 960, height: 540 }, settings: null });
    const { page, world } = battle;
    for (let i = 0; i < 40 && !(await world.evaluate(() => self.__server.actor.unit.alive)); i++) await battle.frames(30);
    const classes = await world.evaluate(() => { const seen = {}; self.__bf.live.turrets.forEach((t, i) => { if (!(t.desc.class in seen)) seen[t.desc.class] = i; }); return seen; });
    for (const [cls, index] of Object.entries(classes)) {
      await world.evaluate((i) => {
        const b = self.__bf, t = b.live.turrets[i], u = self.__server.actor.unit;
        t.setHealth(t.maxHealth); b.turretSeats.leave(u, { placement: false }); b.turretSeats.man(u, t);
        // Every round the seats fire, as it leaves.
        const ordnance = b.turretSeats.ordnance;
        if (!ordnance.__probe) {
          const fire = ordnance.fire.bind(ordnance);
          ordnance.__probe = [];
          ordnance.fire = (def, origin, dir, ...rest) => { ordnance.__probe.push({ origin: origin.toArray(), dir: dir.toArray() }); return fire(def, origin, dir, ...rest); };
        }
      }, index);
      for (const first of [false, true]) {
        for (const aimPitch of pitches) for (const range of ranges) {
          // Set the aim on both hosts, then let the gun and camera settle.
          const level = `(t) => { t.yaw = 0; t.pitch = ${aimPitch === 'low' ? 't.pitchLimits[0]' : aimPitch === 'high' ? 't.pitchLimits[1]' : '0'}; t.yawVel = 0; t.pitchVel = 0; }`;
          await world.evaluate(({ i, level }) => { (0, eval)(`(${level})`)(self.__bf.live.turrets[i]); self.__bf.turretSeats.ordnance.__probe.length = 0; }, { i: index, level });
          await page.evaluate(async ({ first, level }) => {
            const b = __bf; b.keys.clear(); b.setPerspective(first);
            for (let k = 0; k < 20; k++) { if (b.gunner.turret) (0, eval)(`(${level})`)(b.gunner.turret); await b.lockstep(1 / 60, false); }
          }, { first, level });
          // The wall: a 60 m square across the aim, `range` metres out from
          // the drawn camera.
          const camera = await page.evaluate(() => __bf.camera.position.toArray());
          const wall = await world.evaluate(({ i, range, camera }) => {
            const b = self.__bf, t = b.live.turrets[i];
            const from = t.position.clone().fromArray(camera), dir = t.viewDirection(t.position.clone());
            const centre = from.clone().addScaledVector(dir, range);
            const side = dir.clone().cross(t.position.clone().set(0, 1, 0)).normalize(), up = side.clone().cross(dir).normalize();
            const corner = (a, c) => centre.clone().addScaledVector(side, 30 * a).addScaledVector(up, 30 * c);
            const p = [corner(-1, -1), corner(1, -1), corner(1, 1), corner(-1, 1)].flatMap((v) => v.toArray());
            if (b.__probeWall) b.collision.removeBody?.(b.__probeWall);
            b.__probeWall = b.collision.addBody(new Float32Array(p), new Uint32Array([0, 1, 2, 0, 2, 3]), new Uint32Array(2));
            return { centre: centre.toArray(), normal: dir.toArray(), points: p };
          }, { i: index, range, camera });
          // The same wall on the page, whose reticle aims at what it meets.
          await page.evaluate((p) => {
            const b = __bf;
            if (b.__probeWall) b.collision.removeBody?.(b.__probeWall);
            b.__probeWall = b.collision.addBody(new Float32Array(p), new Uint32Array([0, 1, 2, 0, 2, 3]), new Uint32Array(2));
          }, wall.points);
          await page.evaluate(async () => { const b = __bf; for (let k = 0; k < 120; k++) await b.lockstep(1 / 60, false); });
          // Fire: a few rounds' worth of held trigger.
          await page.evaluate(async () => {
            const b = __bf; b.keys.add('Mouse0'); b.pending.firePrimary = true;
            for (let k = 0; k < 30; k++) await b.lockstep(1 / 60, false);
            // A charged gun (the dish turret's) fires as it is let go.
            b.keys.clear();
            for (let k = 0; k < 20; k++) await b.lockstep(1 / 60, k === 19);
          });
          // Whether the gun was turned to a point on the wall (else something
          // nearer stood in the way, and the rounds rightly went there).
          const aimed = await world.evaluate(() => self.__bf.turretSeats.target.toArray());
          const n = wall.normal, c = wall.centre;
          const onWall = Math.abs((aimed[0] - c[0]) * n[0] + (aimed[1] - c[1]) * n[1] + (aimed[2] - c[2]) * n[2]) < 0.05;
          const view = await page.evaluate(() => {
            const b = __bf, cam = b.camera, r = b.hud.reticle;
            if (!r) return null;
            const V = cam.position.constructor;
            const p = new V(2 * r.x - 1, 1 - 2 * r.y, 0.5).unproject(cam).sub(cam.position).normalize();
            return { cam: cam.position.toArray(), ray: p.toArray() };
          });
          const rounds = await world.evaluate(() => self.__bf.turretSeats.ordnance.__probe.slice());
          if (out && !first && range === ranges[0] && aimPitch === pitches[0]) await page.screenshot({ path: `${out}/${map}-${cls}-aim.png` });
          const hitPlane = (o, d) => {
            const n = wall.normal, c = wall.centre;
            const den = d[0] * n[0] + d[1] * n[1] + d[2] * n[2];
            if (Math.abs(den) < 1e-9) return null;
            const t = ((c[0] - o[0]) * n[0] + (c[1] - o[1]) * n[1] + (c[2] - o[2]) * n[2]) / den;
            return t > 0 ? o.map((v, k) => v + d[k] * t) : null;
          };
          const reticle = view && hitPlane(view.cam, view.ray);
          const misses = rounds.map((round) => {
            const at = hitPlane(round.origin, round.dir);
            if (!at || !reticle) return null;
            const m = Math.hypot(...at.map((v, k) => v - reticle[k]));
            const a = at.map((v, k) => v - view.cam[k]), b = reticle.map((v, k) => v - view.cam[k]);
            const cos = (a[0] * b[0] + a[1] * b[1] + a[2] * b[2]) / (Math.hypot(...a) * Math.hypot(...b));
            return { metres: +m.toFixed(3), degrees: +(Math.acos(Math.min(1, cos)) * 180 / Math.PI).toFixed(3) };
          }).filter(Boolean);
          const worst = misses.reduce((w, m) => (m.metres > (w?.metres ?? -1) ? m : w), null);
          // Wherever the aim met something (the wall, or anything nearer):
          // how far that point lies off the reticle's ray and off each
          // round's line.
          const off = (o, d) => { const v = aimed.map((x, k) => x - o[k]); const t = v[0] * d[0] + v[1] * d[1] + v[2] * d[2]; return Math.hypot(...v.map((x, k) => x - d[k] * t)); };
          const reticleOff = view ? +off(view.cam, view.ray).toFixed(3) : null;
          const roundOff = rounds.length ? +Math.max(...rounds.map((r) => off(r.origin, r.dir))).toFixed(3) : null;
          const aimRange = view ? +Math.hypot(...aimed.map((x, k) => x - view.cam[k])).toFixed(1) : null;
          console.log(JSON.stringify({ map, cls, first, pitch: aimPitch, range, onWall, aimRange, reticleOff, roundOff, rounds: rounds.length, worst: onWall ? worst : null }));
        }
      }
      await world.evaluate(() => { const b = self.__bf; if (b.__probeWall) { b.collision.removeBody?.(b.__probeWall); b.__probeWall = null; } b.turretSeats.leave(self.__server.actor.unit); });
      await page.evaluate(async () => { const b = __bf; if (b.__probeWall) { b.collision.removeBody?.(b.__probeWall); b.__probeWall = null; } for (let i = 0; i < 3; i++) await b.lockstep(1 / 60, false); });
    }
    console.log(JSON.stringify({ map, errors: battle.errors.slice(0, 5) }));
    await battle.close();
  }
} finally {
  await browser.close();
}
