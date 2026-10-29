// Requires Vite plus the extracted Naboo: Theed map and all four armies.
// Each army's sniper rifle (its ordnance names a FadeOutTime of 1 s) fired
// in the world, in its worker, stepped in lockstep with the page. The page
// draws the round from the world's records: in flight, then where it
// stopped, fading out and thinning over its FadeOutTime (src/ordnance.js),
// and gone after it. Third and first person, both
// bolt looks, and a round fired from 60 m out at the ground before the camera.
// SNIPER SHOTS is held Classic here: its Enhanced look (sniper-trace.js)
// lingers as vapour past the FadeOutTime by design.
//
// SNIPER_TEST_URL is the Vite server; SNIPER_TEST_ARTIFACTS, if set, gets a
// PNG of each captured frame.
import fs from 'node:fs/promises';
import { launchBrowser, openBattle } from './battle-page.mjs';
const base = process.env.SNIPER_TEST_URL || 'http://127.0.0.1:18449';
const artifacts = process.env.SNIPER_TEST_ARTIFACTS;
const armies = (process.env.SNIPER_TEST_ARMIES || 'rep,cis,imp,all').split(',').map((side) => [side, {
  rep: 'rep_inf_clone_sharpshooter', cis: 'cis_inf_assassindroid', imp: 'imp_inf_scout_trooper', all: 'all_inf_marksman' }[side]]);
// Seconds after the end of the flight at which a frame is captured.
const AFTER = [0, .25, .5, .9, 1.2];
const failures = [];
const browser = await launchBrowser();
try {
  for (const [side, unit] of armies) {
    const battle = await openBattle(browser, { base, query: `map=nab2&side=${side}&unit=${unit}&ai=off`, viewport: { width: 640, height: 400 } });
    const { page, world, errors } = battle;
    page.on('console', (m) => { if (m.type() !== 'error' && /GL_INVALID|WebGL.*error/.test(m.text())) errors.push(m.text()); });
    await page.evaluate(() => { __bf.settings.set('upscaler', 'off'); });
    // Turn him where his aim, a little down, meets open ground 12 to 30 m
    // out, under a clear line of 70 m for the round fired at the camera.
    const stance = await world.evaluate(() => {
      const b = self.__bf, T = b.THREE, s = b.soldier, hit = { point: new T.Vector3(), normal: new T.Vector3() };
      const eye = s.position.clone(); eye.y += 1.6;
      for (let k = 0; k < 72; k++) {
        const yaw = k * Math.PI / 36, level = new T.Vector3(Math.sin(yaw), 0, Math.cos(yaw));
        if (b.collision.cast(eye, level, 70, hit)) continue;
        const down = new T.Vector3(Math.sin(yaw) * Math.cos(.1), -Math.sin(.1), Math.cos(yaw) * Math.cos(.1));
        const ground = b.collision.cast(eye, down, 40, hit);
        if (ground && ground.distance > 12 && ground.distance < 30 && hit.normal.y > .7) return { yaw, pitch: -.1, distance: ground.distance };
      }
      return null;
    });
    if (!stance) { failures.push(`${side}: no open ground in reach`); await battle.close(); continue; }
    for (const look of ['classic', 'enhanced']) for (const view of ['third', 'first', 'camera']) {
      await page.evaluate(({ look, first }) => { const b = __bf; b.settings.set('boltEffects', look); b.settings.set('sniperEffects', 'classic'); b.setPerspective(first); }, { look, first: view === 'first' });
      // Every earlier round gone, the rifle ready and his aim back where it
      // was (each shot kicks it up): turned with mouse counts, at what a
      // count turns him (lookRaw on a copy of his body), as his own input.
      await battle.frames(60);
      await page.evaluate(async ({ yaw, pitch }) => {
        const b = __bf;
        for (let i = 0; i < 8; i++) {
          const s = b.soldier, probe = Object.create(s);
          probe.aimYaw = probe.aimPitch = 0; probe.lookRaw(1, 1, b.settings.mouseScale, false);
          const dyaw = Math.atan2(Math.sin(yaw - s.aimYaw), Math.cos(yaw - s.aimYaw)), dpitch = pitch - s.aimPitch;
          if (Math.abs(dyaw) < 2e-3 && Math.abs(dpitch) < 2e-3) break;
          b.controls.looks.push([probe.aimYaw ? dyaw / probe.aimYaw : 0, probe.aimPitch ? dpitch / probe.aimPitch : 0]);
          for (let k = 0; k < 4; k++) await b.lockstep(1 / 60, false);
        }
      }, stance);
      await battle.frames(30);
      const before = await page.evaluate(() => Math.max(0, ...__bf.client.entity.combat.records.keys()));
      if (view === 'camera') {
        const at = await page.evaluate(() => { const c = __bf.camera, f = new __bf.THREE.Vector3(); c.getWorldDirection(f); f.y = 0; f.normalize(); return { eye: c.position.toArray(), forward: f.toArray() }; });
        await world.evaluate(({ eye, forward }) => {
          const b = self.__bf, T = b.THREE, def = b.combat.arsenal.weapons.find((w) => w.def.ordnance?.FadeOutTime)?.def.ordnance;
          const e = new T.Vector3(...eye), f = new T.Vector3(...forward), side = new T.Vector3(f.z, 0, -f.x);
          const from = e.clone().addScaledVector(f, 60).addScaledVector(side, 4); from.y += 1.5;
          // The ground 8 m before the camera.
          const hit = { point: new T.Vector3(), normal: new T.Vector3() }, above = e.clone().addScaledVector(f, 8); above.y += 5;
          const to = b.collision.cast(above, new T.Vector3(0, -1, 0), 50, hit) ? hit.point.clone() : above.setY(above.y - 8);
          b.ordnance.fire(def, from, to.sub(from).normalize(), null);
        }, at);
      } else {
        await page.evaluate(async () => { const b = __bf; b.keys.add('Mouse0'); await b.lockstep(1 / 60, false); b.keys.delete('Mouse0'); });
      }
      const result = await page.evaluate(async ({ AFTER, before, keep }) => {
        const b = __bf, g = b.graphics, gl = b.renderer.getContext();
        const w = b.renderer.domElement.width, h = b.renderer.domElement.height;
        const pixels = () => { const p = new Uint8Array(w * h * 4); gl.readPixels(0, 0, w, h, gl.RGBA, gl.UNSIGNED_BYTE, p); return p; };
        // The light added over the same frame without the bolt batches.
        const capture = () => {
          b.water?.reflect(b.camera); g.render(0); const shown = pixels(), image = keep ? b.renderer.domElement.toDataURL() : null;
          const batches = [...b.ordnance.renderer.batches.values()], visible = batches.map((x) => x.mesh.visible);
          for (const x of batches) x.mesh.visible = false;
          g.render(0); const hidden = pixels();
          batches.forEach((x, i) => { x.mesh.visible = visible[i]; });
          let changed = 0, energy = 0;
          for (let i = 0; i < shown.length; i += 4) {
            const d = Math.max(0, shown[i] - hidden[i]) + Math.max(0, shown[i + 1] - hidden[i + 1]) + Math.max(0, shown[i + 2] - hidden[i + 2]);
            if (d > 12) changed++;
            energy += d;
          }
          return { changed, energy, image };
        };
        // His round as this page has it from the world: in flight, then
        // once its terminal has arrived, where it stopped.
        const round = () => [...b.client.entity.combat.records].filter(([id, r]) => id > before && r.bolt?.def?.FadeOutTime).map(([, r]) => r).at(-1);
        const frames = [];
        let record = null;
        for (let i = 0; i < 30 && !record; i++) {
          await b.lockstep(1 / 60, false);
          const r = round();
          if (r?.schema === 2) record = r;
          else if (r) { await b.lockstep(0, true); frames.push({ after: -1, tick: b.worldClock.tick, ...capture() }); }
        }
        if (!record) return { error: 'no streak after the round stopped', frames };
        const bolt = record.bolt, end = bolt.networkFlightTick + bolt.life * 60;
        for (const after of AFTER) {
          const tick = Math.ceil(end + after * 60);
          while (b.worldClock.tick < tick) await b.lockstep(1 / 60, false);
          await b.lockstep(0, true);
          frames.push({ after, tick: b.worldClock.tick, ...capture() });
        }
        return { distance: bolt.life * bolt.speed, frames };
      }, { AFTER, before, keep: Boolean(artifacts) });
      if (artifacts) {
        await fs.mkdir(artifacts, { recursive: true });
        for (const [i, frame] of result.frames.entries()) await fs.writeFile(`${artifacts}/sniper-${side}-${look}-${view}-${i}${frame.after < 0 ? '-flight' : `-${Math.round(frame.after * 100)}`}.png`, Buffer.from(frame.image.split(',')[1], 'base64'));
      }
      const shot = { side, look, view, distance: Math.round(result.distance ?? 0), frames: result.frames.map(({ after, changed, energy }) => ({ after, changed, energy })) };
      console.log(JSON.stringify(shot));
      if (result.error) { failures.push(`${side} ${look} ${view}: ${result.error}`); continue; }
      const [atEnd, , half, late, gone] = result.frames.filter((frame) => frame.after >= 0);
      if (atEnd.changed < (view === 'camera' ? 20 : 3)) failures.push(`${side} ${look} ${view}: no streak where the round stopped`);
      if (!(half.changed > 0 && half.energy < atEnd.energy)) failures.push(`${side} ${look} ${view}: the streak does not fade`);
      if (!(late.energy < half.energy)) failures.push(`${side} ${look} ${view}: the streak does not keep fading`);
      if (gone.changed > 2) failures.push(`${side} ${look} ${view}: the streak outlives its FadeOutTime`);
    }
    if (errors.length) failures.push(`${side}: ${errors.join(' | ')}`);
    await battle.close();
  }
} finally { await browser.close(); }
if (failures.length) { console.error(failures.join('\n')); process.exit(1); }
console.log('sniper bolts: every army\'s round fades where it stops');
