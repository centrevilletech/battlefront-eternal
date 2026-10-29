// Close-ups of turret gunners: for each turret class on the given maps, the
// player and then a bot mans one, and the page's camera is put beside, in
// front of and behind the gun (in the seat node's frame) and drawn once, with the
// gunner's hit shapes (the world's, as rounds meet him) drawn over his body
// as wireframes. The bot is drawn as the page draws every other unit, from
// the world's packets, which is how a remote player's gunner is drawn too.
// Also prints, per gunner, the drawn joints and the hit shapes in the seat
// node's frame (x left, y up, z along the gun), how far each drawn joint
// lies outside every shape, and the share of the drawn body's vertices
// within 2 cm of one (coverage, with the uncovered vertices by bone).
//
//   PLAYWRIGHT_MODULE=... TURRET_TEST_URL=http://127.0.0.1:18495 TURRET_PROBE_OUT=/tmp/shots \
//     node tools/turret-body-probe.mjs kas2,hot1 [pitch-degrees]
import fs from 'node:fs/promises';
import { launchBrowser, openBattle } from '../tests/battle-page.mjs';

const base = process.env.TURRET_TEST_URL || 'http://127.0.0.1:18493';
const out = process.env.TURRET_PROBE_OUT || '/tmp/bfe-turret-body-probe';
const maps = (process.argv[2] || 'kas2,hot1').split(',');
const pitchDeg = Number(process.argv[3] || 0);
const cw = { side: 'rep', unit: 'rep_inf_clone_trooper' }, gcw = { side: 'imp', unit: 'imp_inf_storm_trooper' }, snow = { side: 'imp', unit: 'imp_inf_storm_troopersnow' };
const eras = { hot1: snow, tat2: gcw, yav1: gcw, end1: gcw, bes2: gcw, tat1: cw, kas2: cw, geo1: cw, bes1: gcw, nab2: cw, kam1: cw, rhn1: gcw, rhn2: gcw, nab1: cw, yav2: gcw };
// Viewpoints in the seat node's frame (x left, y up, z along the gun), from and at.
const VIEWS = { side: [[2.6, 1.5, 0.1], [0, 1.25, 0]], front: [[0.4, 1.9, 2.4], [0, 1.25, 0]], back: [[0.6, 2.4, -2.6], [0, 1.2, 0.2]] };
await fs.mkdir(out, { recursive: true });

// In the world: the gunner's hit shapes, and where they lie in the seat
// node's frame.
const measure = () => {
  const b = self.__bf, T = b.THREE;
  return (i, id) => {
    const t = b.live.turrets[i];
    const u = id === 'player' ? self.__server.actor.unit : b.units.list.find((x) => x.id === id);
    const seat = (t.seat || t.rig.root).getWorldPosition(new T.Vector3());
    const turn = (t.seat || t.rig.root).getWorldQuaternion(new T.Quaternion());
    const inv = new T.Matrix4().compose(seat, turn, new T.Vector3(1, 1, 1)).invert();
    const r = (v) => v.clone().applyMatrix4(inv).toArray().map((x) => +x.toFixed(3));
    return { seat: seat.toArray(), shapes: u.hitShapes().map((s) => ({ part: s.part, type: s.type, center: s.center.toArray(), half: s.halfSize?.toArray(), radius: s.radius, local: r(s.center) })) };
  };
};

const browser = await launchBrowser();
try {
  for (const map of maps) {
    const { side, unit } = eras[map] || cw;
    const battle = await openBattle(browser, { base, query: `map=${map}&side=${side}&unit=${unit}&shadows=low`, viewport: { width: 800, height: 600 }, settings: null });
    const { page, world } = battle;
    for (let i = 0; i < 40 && !(await world.evaluate(() => self.__server.actor.unit.alive)); i++) await battle.frames(30);
    const classes = await world.evaluate(() => { const seen = {}; self.__bf.live.turrets.forEach((t, i) => { if (!(t.desc.class in seen)) seen[t.desc.class] = i; }); return seen; });
    console.log(map, classes);
    for (const [cls, index] of Object.entries(classes)) {
      for (const who of ['player', 'bot']) {
        if (who === 'bot') await world.evaluate(() => self.__bf.factory.prepare(self.__server.actor.unit.className));
        const id = await world.evaluate(({ i, who, pitch }) => {
          const b = self.__bf, t = b.live.turrets[i];
          t.setHealth(t.maxHealth);
          const player = self.__server.actor.unit;
          b.turretSeats.leave(player, { placement: false });
          if (t.lease) b.turretSeats.leave(t.lease.unit, { placement: false });
          let u = player;
          if (who === 'bot') {
            // A bot of the player's class, fresh from the factory (the battle's
            // own may not have spawned yet).
            u = self.__probeBot;
            if (!u || !u.alive) {
              u = self.__probeBot = b.factory.acquire(player.className);
              u.team = u.combat.team = player.team;
              u.spawn(t.position.x + 2, t.position.y + 1, t.position.z, 0);
            }
            player.soldier.spawn(t.position.x + 30, t.position.y + 30, t.position.z + 30, 0);
          }
          if (!b.turretSeats.man(u, t)) return null;
          t.pitch = pitch * Math.PI / 180;
          return who === 'bot' ? u.id : 'player';
        }, { i: index, who, pitch: pitchDeg });
        if (id === null) { console.log(map, cls, who, 'not manned'); continue; }
        await page.evaluate(async () => { const b = __bf; b.keys.clear(); b.setPerspective(false); for (let i = 0; i < 30; i++) await b.lockstep(1 / 60, false); });
        // Hold the pitch while the frames run (the player's own steer eases it).
        await world.evaluate(({ i, pitch }) => { const t = self.__bf.live.turrets[i]; t.pitch = pitch * Math.PI / 180; t.pitchVel = 0; t.yawVel = 0; }, { i: index, pitch: pitchDeg });
        await page.evaluate(async () => { for (let i = 0; i < 4; i++) await __bf.lockstep(1 / 60, false); });
        const m = await world.evaluate(`(${measure})()(${index}, ${JSON.stringify(id)})`);
        const drawn = await page.evaluate(({ i, id, views, shapes }) => {
          const b = __bf, T = b.THREE;
          const t = b.live.turrets[i];
          const u = id === 'player' ? b.playerUnit : b.units.list.find((x) => x.id === id);
          if (!u) return { error: 'no unit on the page' };
          b.scene.updateMatrixWorld(true);
          const seat = (t.seat || t.rig.root).getWorldPosition(new T.Vector3());
          const turn = (t.seat || t.rig.root).getWorldQuaternion(new T.Quaternion());
          const frame = new T.Matrix4().compose(seat, turn, new T.Vector3(1, 1, 1));
          const inv = frame.clone().invert();
          const bones = {}, outside = {};
          // How far a drawn joint lies outside every hit shape (0: inside one).
          const away = (p) => {
            let best = Infinity;
            for (const s of shapes) {
              const c = new T.Vector3().fromArray(s.center);
              let d;
              if (s.type === 'capsule') {
                const h = new T.Vector3().fromArray(s.half), l2 = h.lengthSq();
                const k = l2 > 1e-12 ? Math.max(-1, Math.min(1, p.clone().sub(c).dot(h) / l2)) : 0;
                d = p.distanceTo(c.clone().addScaledVector(h, k)) - s.radius;
              } else if (s.type === 'sphere') d = p.distanceTo(c) - s.radius;
              else continue;
              best = Math.min(best, Math.max(0, d));
            }
            return +best.toFixed(3);
          };
          for (const n of ['bone_head', 'bone_neck', 'bone_pelvis', 'bone_l_hand', 'bone_r_hand', 'bone_l_forearm', 'bone_r_forearm', 'bone_l_calf', 'bone_r_calf', 'bone_l_foot', 'bone_r_foot', 'hp_weapons']) {
            const bone = u.actor.byName[n];
            if (!bone) continue;
            const p = bone.getWorldPosition(new T.Vector3());
            bones[n] = p.clone().applyMatrix4(inv).toArray().map((x) => +x.toFixed(3));
            if (n !== 'hp_weapons') outside[n] = away(p);
          }
          // The share of the drawn body's vertices inside some hit shape, and
          // the farthest one outside them all (the body as drawn this frame).
          let inside = 0, total = 0, worst = 0;
          const missed = {};
          u.actor.root.updateMatrixWorld(true);
          u.actor.root.traverse((o) => {
            if (!o.isSkinnedMesh || !o.visible) return;
            const pos = o.geometry.attributes.position, si = o.geometry.attributes.skinIndex, v = new T.Vector3();
            for (let k = 0; k < pos.count; k++) {
              v.fromBufferAttribute(pos, k); o.applyBoneTransform(k, v); v.applyMatrix4(o.matrixWorld);
              const d = away(v); total++; if (d <= 0.02) inside++; worst = Math.max(worst, d);
              if (d > 0.02) { const bone = o.skeleton.bones[si.getX(k)]?.name || '?'; missed[bone] = (missed[bone] || 0) + 1; }
            }
          });
          const coverage = { inside: +(inside / Math.max(total, 1)).toFixed(3), worst: +worst.toFixed(3), vertices: total, missed };
          // Wireframes of the world's hit shapes.
          const group = new T.Group();
          const mat = new T.MeshBasicMaterial({ color: 0xff2020, wireframe: true, depthTest: false, transparent: true, opacity: 0.6 });
          for (const s of shapes) {
            const c = new T.Vector3().fromArray(s.center);
            if (s.type === 'sphere') { const g = new T.Mesh(new T.SphereGeometry(s.radius, 10, 8), mat); g.position.copy(c); group.add(g); }
            else if (s.type === 'capsule') {
              const h = new T.Vector3().fromArray(s.half);
              const g = new T.Mesh(new T.CapsuleGeometry(s.radius, 2 * h.length(), 4, 8), mat);
              g.position.copy(c);
              if (h.lengthSq() > 1e-9) g.quaternion.setFromUnitVectors(new T.Vector3(0, 1, 0), h.clone().normalize());
              group.add(g);
            }
          }
          group.renderOrder = 999;
          const urls = {};
          for (const overlay of [false, true]) {
            if (overlay) b.scene.add(group);
            for (const [name, [from, at]] of Object.entries(views)) {
              const eye = new T.Vector3().fromArray(from).applyMatrix4(frame), look = new T.Vector3().fromArray(at).applyMatrix4(frame);
              const saved = { p: b.camera.position.clone(), q: b.camera.quaternion.clone(), fov: b.camera.fov };
              b.camera.position.copy(eye); b.camera.lookAt(look); b.camera.fov = 50; b.camera.updateProjectionMatrix(); b.camera.updateMatrixWorld(true);
              b.graphics.render(0);
              urls[`${name}${overlay ? '-shapes' : ''}`] = b.renderer.domElement.toDataURL();
              b.camera.position.copy(saved.p); b.camera.quaternion.copy(saved.q); b.camera.fov = saved.fov; b.camera.updateProjectionMatrix();
            }
          }
          b.scene.remove(group);
          return { bones, outside, coverage, urls, visible: u.actor.root.visible, clip: u.animator?.gunning?.clip?.name ?? null,
            weaponShown: (() => { let shown = false; u.actor.root.traverse((o) => { if (o.userData?.weapon && o.visible) shown = true; }); return shown; })() };
        }, { i: index, id, views: VIEWS, shapes: m.shapes });
        if (drawn.error) { console.log(map, cls, who, drawn.error); continue; }
        for (const [name, url] of Object.entries(drawn.urls)) await fs.writeFile(`${out}/${map}-${cls}-${who}-${name}.png`, Buffer.from(url.split(',')[1], 'base64'));
        console.log(JSON.stringify({ map, cls, who, clip: drawn.clip, visible: drawn.visible, coverage: drawn.coverage, outside: drawn.outside, bones: drawn.bones, shapes: m.shapes.map((s) => [s.part, s.type, s.local, +s.radius.toFixed(2)]) }));
      }
      await world.evaluate((i) => { const b = self.__bf, t = b.live.turrets[i]; if (t.lease) b.turretSeats.leave(t.lease.unit, { placement: false }); }, index);
      await page.evaluate(async () => { for (let i = 0; i < 3; i++) await __bf.lockstep(1 / 60, false); });
    }
    console.log(JSON.stringify({ map, errors: battle.errors.slice(0, 5) }));
    await battle.close();
  }
} finally { await browser.close(); }
