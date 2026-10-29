// The Enhanced sky's flyers (Graphics > Ocean and sky: Enhanced) beside the
// original's, from the same views, at the default view distance
// (src/sky-flyers.js, docs/environment-effects.md "The sky's flyers").
//
// With Vite running and maps prepared since maps generation 16 (set
// PLAYWRIGHT_MODULE if Playwright is elsewhere):
//   node tests/sky-flyers-enhanced-browser.mjs
// Env: GAME_URL (http://127.0.0.1:18449), SKY_FLYERS_OUTPUT (frames, default
// /tmp/bf-sky-flyers/enhanced), SKY_FLYERS_ONLY (comma list of the names
// below), SKY_FLYERS_SIZE (960x540).
//
// For each battle the page builds the original's classes and the era's,
// flies them, and stands the camera where it shows the most of an Enhanced
// ship (a camera shot, a post, or over the posts); Enhanced must show it
// there, and the same view in Classic (the setting changed live) shows only
// what the original would. On Geonosis a gunship is followed from its start
// to its end in each mode: Classic's jumps to full alpha when it leaves its
// ring before it has faded in, Enhanced's is solid inside the ring and
// fades without a jump. Then a droid fighter crossing a narrow view under
// the temporal upscaler: with its motion drawn it is where it is, not a few
// pixels behind.
import { mkdirSync, writeFileSync } from 'node:fs';
import assert from 'node:assert/strict';
import { launchBrowser, openBattle, LOW_SETTINGS } from './battle-page.mjs';

const URL = process.env.GAME_URL || 'http://127.0.0.1:18449';
const OUT = process.env.SKY_FLYERS_OUTPUT || '/tmp/bf-sky-flyers/enhanced';
const ONLY = (process.env.SKY_FLYERS_ONLY || '').split(',').filter(Boolean);
const [WIDTH, HEIGHT] = (process.env.SKY_FLYERS_SIZE || '960x540').split('x').map(Number);
mkdirSync(OUT, { recursive: true });

const CW = { rep_fly_assault_dome: 2, cis_fly_fedlander_dome: 2 };
const GCW = { all_fly_moncalamari_dome: 2, imp_fly_destroyer_dome: 2 };
const REP = 'side=rep&unit=rep_inf_clone_trooper', IMP = 'side=imp&unit=imp_inf_storm_trooper';
const BATTLES = [
  { name: 'bespin-platforms-cw', query: `map=bes1&${REP}`, classic: {}, enhanced: CW },
  { name: 'bespin-platforms-gcw', query: `map=bes1&${IMP}`, classic: {}, enhanced: GCW },
  { name: 'cloud-city-cw', query: `map=bes2&${REP}`, classic: { rep_fly_assault_dome: 2 }, enhanced: CW },
  { name: 'cloud-city-gcw', query: `map=bes2&${IMP}`, classic: {}, enhanced: GCW },
  { name: 'mos-eisley-gcw', query: `map=tat2&${IMP}`, classic: { imp_fly_destroyer_dome: 2 }, enhanced: GCW },
  { name: 'naboo-plains-gcw', query: `map=nab1&${IMP}`, classic: { imp_fly_destroyer_dome: 1 }, enhanced: { imp_fly_destroyer_dome: 1 } },
  { name: 'naboo-plains-cw', query: `map=nab1&${REP}`, classic: { cis_fly_fedlander_dome: 1 }, enhanced: { cis_fly_fedlander_dome: 1 } },
  { name: 'geonosis', query: `map=geo1&${REP}`, geonosis: true },
].filter((battle) => !ONLY.length || ONLY.includes(battle.name));

// On the page: the classes, views, frames and pixel comparisons.
function pageHelpers() {
  const sky = () => __bf.skyFlyers;
  window.__sky = {
    classes(mode) {
      return Object.fromEntries((sky()?.classes || []).filter((kind) => kind[mode]).map((kind) => [kind.def.model, kind.ships.length]));
    },
    mode(mode) {
      __bf.settings.set('oceanEffects', mode);
      return sky().enhanced;
    },
    fly(seconds, dt = 1 / 30) {
      for (let t = 0; t < seconds - 1e-9; t += dt) sky().update(dt, __bf.camera);
    },
    eyes() {
      const shots = (__bf.spawnPort?.cameraShots?.() || []).map((s) => s.position);
      const posts = (__bf.posts?.posts || []).map((p) => p.position);
      const eyes = [...shots, ...posts].map((p) => [p.x ?? p[0], (p.y ?? p[1]) + 2, p.z ?? p[2]]);
      if (posts.length) {
        const mid = (k) => posts.reduce((sum, p) => sum + p[k], 0) / posts.length;
        eyes.push([mid('x'), Math.max(...posts.map((p) => p.y)) + 40, mid('z')]);
      }
      return eyes;
    },
    // The camera at `eye` looking at `at`, a little under it so the ground
    // shows, `fov` degrees.
    aim(eye, at, fov) {
      const camera = __bf.camera;
      camera.fov = fov;
      camera.updateProjectionMatrix();
      camera.position.set(...eye);
      const across = Math.hypot(at[0] - eye[0], at[2] - eye[2]);
      camera.lookAt(at[0], at[1] - 0.3 * across * Math.tan(fov * Math.PI / 360), at[2]);
      camera.updateMatrixWorld();
      sky().update(0, camera);
    },
    grab() {
      const { graphics, renderer } = __bf;
      graphics.render(1);
      const gl = renderer.getContext();
      const w = gl.drawingBufferWidth, h = gl.drawingBufferHeight;
      const pixels = new Uint8Array(w * h * 4);
      gl.readPixels(0, 0, w, h, gl.RGBA, gl.UNSIGNED_BYTE, pixels);
      return { pixels, w, h, url: renderer.domElement.toDataURL('image/png') };
    },
    // Pixels the ships change: the frame against the same frame without them.
    shipPixels(shown, hidden) {
      let changed = 0, sx = 0, sy = 0;
      for (let i = 0; i < shown.w * shown.h; i++) {
        const k = i * 4;
        if (Math.abs(shown.pixels[k] - hidden.pixels[k]) + Math.abs(shown.pixels[k + 1] - hidden.pixels[k + 1]) + Math.abs(shown.pixels[k + 2] - hidden.pixels[k + 2]) > 30) {
          changed++;
          sx += i % shown.w;
          sy += Math.floor(i / shown.w);
        }
      }
      return { changed, x: changed ? sx / changed : null, y: changed ? sy / changed : null };
    },
    // As shipPixels, but only the pixels within `reach` of those `near`
    // marks (another frame's ship pixels): what the upscaler does to the
    // rest of the sky is no ship.
    shipPixelsNear(shown, hidden, near, reach = 24) {
      const { w, h } = shown, mask = new Uint8Array(w * h), wide = new Uint8Array(w * h);
      for (let i = 0; i < w * h; i++) mask[i] = near.mask[i];
      // Grown by `reach` across, then down.
      for (let y = 0; y < h; y++) for (let x = 0, last = -1e9; x < w; x++) { if (mask[y * w + x]) last = x; wide[y * w + x] = x - last <= reach ? 1 : 0; }
      for (let y = 0; y < h; y++) for (let x = w - 1, last = 1e9; x >= 0; x--) { if (mask[y * w + x]) last = x; if (last - x <= reach) wide[y * w + x] = 1; }
      mask.set(wide);
      for (let x = 0; x < w; x++) {
        for (let y = 0, last = -1e9; y < h; y++) { if (mask[y * w + x]) last = y; wide[y * w + x] = y - last <= reach ? 1 : 0; }
        for (let y = h - 1, last = 1e9; y >= 0; y--) { if (mask[y * w + x]) last = y; if (last - y <= reach) wide[y * w + x] = 1; }
      }
      let changed = 0, sx = 0, sy = 0;
      for (let i = 0; i < w * h; i++) {
        if (!wide[i]) continue;
        const k = i * 4;
        if (Math.abs(shown.pixels[k] - hidden.pixels[k]) + Math.abs(shown.pixels[k + 1] - hidden.pixels[k + 1]) + Math.abs(shown.pixels[k + 2] - hidden.pixels[k + 2]) > 30) {
          changed++;
          sx += i % w;
          sy += Math.floor(i / w);
        }
      }
      return { changed, x: changed ? sx / changed : null, y: changed ? sy / changed : null };
    },
    // The pixels the ships change, as a mask.
    shipMask(shown, hidden) {
      const mask = new Uint8Array(shown.w * shown.h);
      for (let i = 0; i < shown.w * shown.h; i++) {
        const k = i * 4;
        mask[i] = Math.abs(shown.pixels[k] - hidden.pixels[k]) + Math.abs(shown.pixels[k + 1] - hidden.pixels[k + 1]) + Math.abs(shown.pixels[k + 2] - hidden.pixels[k + 2]) > 30 ? 1 : 0;
      }
      return { mask };
    },
    film(eye, at, fov) {
      this.aim(eye, at, fov);
      const shown = this.grab();
      sky().group.visible = false;
      sky().update(0, __bf.camera);
      const hidden = this.grab();
      sky().group.visible = true;
      sky().update(0, __bf.camera);
      return { url: shown.url, hiddenUrl: hidden.url, ...this.shipPixels(shown, hidden) };
    },
    // For each eye, its nearest drawn ship well above it; the nearest few.
    picks(eyes) {
      const found = [];
      for (const eye of eyes) {
        let best = null;
        for (const kind of sky().classes) {
          if (!sky().active(kind)) continue;
          for (const ship of kind.ships) {
            const d = Math.hypot(ship.position.x - eye[0], ship.position.y - eye[1], ship.position.z - eye[2]);
            if (Math.abs(ship.fade) > 0.8 && ship.position.y > eye[1] + 30 && (!best || d < best.distance)) {
              best = { model: kind.def.model, eye, at: ship.position.toArray(), distance: d };
            }
          }
        }
        if (best) found.push(best);
      }
      return found.sort((a, b) => a.distance - b.distance).slice(0, 12);
    },
    // One ship of a class from its start to its end: |fade| and whether it
    // is inside its ring each `step` s, a frame each `every` samples.
    follow(model, eye, step, every) {
      const flyers = sky(), kind = flyers.classes.find((k) => k.def.model === model);
      const ship = kind.ships[0];
      flyers.launch(kind, ship);
      const samples = [], frames = [];
      for (let i = 0; i < 400; i++) {
        const before = ship.fade;
        // Inside the ring the original starts it on.
        const inside = Math.hypot(ship.position.x, ship.position.z) <= kind.def.distance - 0.5;
        samples.push({ alpha: Math.abs(ship.fade), inside, at: ship.position.toArray() });
        if (i % every === 0) {
          const d = Math.hypot(...ship.position.toArray().map((v, k) => v - eye[k]));
          // The same size on screen, whatever its distance.
          const fov = Math.min(60, Math.max(3, 2 * Math.atan(45 / d) * 180 / Math.PI));
          this.aim(eye, ship.position.toArray(), fov);
          frames.push(this.grab().url);
        }
        for (let t = 0; t < step - 1e-9; t += 1 / 60) flyers.update(1 / 60, __bf.camera);
        if (before < 0 && ship.fade >= 0) break;
      }
      return { samples, frames };
    },
  };
}

const save = (file, url) => writeFileSync(file, Buffer.from(url.split(',')[1], 'base64'));

const browser = await launchBrowser();
const failures = [];
try {
  for (const battle of BATTLES) {
    const started = Date.now();
    let opened = null;
    try {
      opened = await openBattle(browser, { base: URL, query: `${battle.query}&seed=7`, viewport: { width: WIDTH, height: HEIGHT },
        settings: { ...LOW_SETTINGS, oceanEffects: 'enhanced', upscaler: 'off', viewDistance: 'original' }, timeout: 600000 });
      const { page } = opened;
      await page.evaluate(pageHelpers);
      if (battle.geonosis) await geonosis(page);
      else await compare(page, battle);
      const errors = opened.errors.filter((e) => /sky|flyer|shader|WebGL|program/i.test(e));
      assert.deepEqual(errors, [], `${battle.name}: errors`);
      console.log(`${battle.name}: passes (${Math.round((Date.now() - started) / 1000)} s)`);
    } catch (error) {
      failures.push(`${battle.name}: ${error.message}`);
      console.log(`FAIL ${battle.name}: ${error.message}`);
    } finally {
      await opened?.close();
    }
  }
} finally {
  await browser.close();
}
if (failures.length) {
  console.log(failures.join('\n'));
  process.exit(1);
}
console.log(`all ${BATTLES.length} pass; frames in ${OUT}`);

// Classic beside Enhanced, from the view that shows the most of an Enhanced ship.
async function compare(page, battle) {
  assert.deepEqual(await page.evaluate(() => __sky.classes('classic')), battle.classic, `${battle.name}: the original's classes`);
  assert.deepEqual(await page.evaluate(() => __sky.classes('enhanced')), battle.enhanced, `${battle.name}: the era's classes`);
  assert.equal(await page.evaluate(() => __sky.mode('enhanced')), true);
  await page.evaluate(() => __sky.fly(10));
  const eyes = await page.evaluate(() => __sky.eyes());
  let picks = [];
  for (let t = 0; t < 120 && !picks.length; t += 2) picks = await page.evaluate(([eyes]) => { const p = __sky.picks(eyes); if (!p.length) __sky.fly(2); return p; }, [eyes]);
  assert.ok(picks.length, `${battle.name}: an Enhanced ship over some view`);
  let best = null;
  for (const pick of picks) {
    const view = await page.evaluate(([eye, at]) => { const v = __sky.film(eye, at, 50); return { changed: v.changed }; }, [pick.eye, pick.at]);
    if (!best || view.changed > best.changed) best = { ...pick, changed: view.changed };
  }
  const enhanced = await page.evaluate(([eye, at]) => __sky.film(eye, at, 50), [best.eye, best.at]);
  assert.equal(await page.evaluate(() => __sky.mode('classic')), false);
  const classic = await page.evaluate(([eye, at]) => __sky.film(eye, at, 50), [best.eye, best.at]);
  await page.evaluate(() => __sky.mode('enhanced'));
  save(`${OUT}/${battle.name}-enhanced.png`, enhanced.url);
  save(`${OUT}/${battle.name}-classic.png`, classic.url);
  console.log(`${battle.name}: ${best.model} ${best.distance.toFixed(0)} m from ${best.eye.map((v) => v.toFixed(0))}; ` +
    `Enhanced ${enhanced.changed} pixels of ships, Classic ${classic.changed}`);
  assert.ok(enhanced.changed > 150, `${battle.name}: Enhanced shows the ship`);
  if (!Object.keys(battle.classic).length) assert.equal(classic.changed, 0, `${battle.name}: Classic has none`);
}

// A gunship from start to end in each mode, then a droid fighter under the
// temporal upscaler.
async function geonosis(page) {
  const counts = { rep_fly_assault_dome: 2, rep_fly_gunship_dome: 20, cis_fly_droidfighter_dome: 20, cis_fly_techounion_dome: 8 };
  assert.deepEqual(await page.evaluate(() => __sky.classes('classic')), counts);
  assert.deepEqual(await page.evaluate(() => __sky.classes('enhanced')), counts);
  const eyes = await page.evaluate(() => __sky.eyes());
  const eye = eyes[eyes.length - 1];
  for (const mode of ['classic', 'enhanced']) {
    await page.evaluate(([mode]) => __sky.mode(mode), [mode]);
    // Classic: a gunship that leaves its ring before it has faded in (most
    // cross it in 3 to 7 s), to show the jump.
    let samples, frames, alphas, jumps;
    for (let attempt = 0; attempt < 12; attempt++) {
      ({ samples, frames } = await page.evaluate(([eye]) => __sky.follow('rep_fly_gunship_dome', eye, 0.25, 4), [eye]));
      alphas = samples.map((s) => s.alpha);
      // A rise faster than the fade-in's 0.0625 a sample is the jump.
      jumps = alphas.slice(1).map((a, i) => a - alphas[i]).filter((d) => d > 0.1);
      if (mode === 'enhanced' || jumps.length) break;
    }
    frames.forEach((url, i) => save(`${OUT}/geonosis-fade-${mode}-${String(i).padStart(2, '0')}.png`, url));
    const peak = alphas.indexOf(Math.max(...alphas));
    console.log(`geonosis ${mode}: alpha every 0.25 s ${alphas.map((a) => a.toFixed(2)).join(' ')}`);
    if (mode === 'classic') {
      // Cut off before it has faded in, it jumps to full and fades from there.
      assert.equal(jumps.length, 1, 'Classic: the jump to full');
    } else {
      assert.equal(jumps.length, 0, 'Enhanced: no jump');
      assert.ok(alphas.slice(peak).every((a, i, list) => i === 0 || a <= list[i - 1] + 1e-9), 'Enhanced: up, then only down');
      assert.ok(samples.filter((s) => s.inside).every((s) => s.alpha > 0.999), 'Enhanced: solid inside its ring');
    }
  }
  // A droid fighter crossing a 15 degree view, under the temporal upscaler:
  // where it is drawn against where it is, with the ships' motion drawn and
  // without it. Geonosis's spires stand in many views, so a fighter counts
  // only when it shows itself, before and after.
  const result = await page.evaluate(async ([eye]) => {
    const { fullScreenPass } = await import('/src/render-passes.js');
    const flyers = __bf.skyFlyers, kind = flyers.classes.find((k) => k.def.model === 'cis_fly_droidfighter_dome');
    const camera = __bf.camera;
    const gl = __bf.renderer.getContext();
    const read = () => { const w = gl.drawingBufferWidth, h = gl.drawingBufferHeight; const pixels = new Uint8Array(w * h * 4); gl.readPixels(0, 0, w, h, gl.RGBA, gl.UNSIGNED_BYTE, pixels); return { pixels, w, h, url: __bf.renderer.domElement.toDataURL('image/png') }; };
    // The view as it is without the upscaler: with the ships, without them,
    // and without this one.
    const still = (ship) => {
      __bf.settings.set('upscaler', 'off');
      const off = __sky.grab();
      flyers.group.visible = false;
      flyers.update(0, camera);
      const hidden = __sky.grab();
      flyers.group.visible = true;
      const fade = ship.fade;
      ship.fade = 0;
      flyers.update(0, camera);
      const others = __sky.grab();
      ship.fade = fade;
      flyers.update(0, camera);
      return { off, hidden, others, own: __sky.shipPixels(off, others).changed, ...__sky.shipPixels(off, hidden) };
    };
    // 24 frames under the temporal upscaler from `start` (the ships' flight
    // saved by snapshot()), the ships' motion drawn or not.
    const snapshot = () => flyers.classes.map((k) => k.ships.map((s) => ({ s, position: s.position.clone(), velocity: [...s.velocity], fade: s.fade, age: s.age, entered: s.entered, matrix: s.matrix.clone(), previous: s.previous.clone() })));
    const restore = (saved) => saved.flat().forEach((v) => { v.s.position.copy(v.position); v.s.velocity = [...v.velocity]; v.s.fade = v.fade; v.s.age = v.age; v.s.entered = v.entered; v.s.matrix.copy(v.matrix); v.s.previous.copy(v.previous); });
    const film = (saved, motion) => {
      restore(saved);
      const meshes = flyers.classes.flatMap((k) => k.motions);
      if (!motion) for (const m of meshes) flyers.motion.removeExtra(m);
      __bf.settings.set('upscaler', 'temporal');
      __bf.graphics.resetHistory?.();
      for (let f = 0; f < 24; f++) { flyers.update(1 / 60, camera); __bf.graphics.render(f / 60); }
      const temporal = read();
      if (!motion) for (const m of meshes) flyers.motion.addExtra(m);
      return temporal;
    };
    for (let tries = 0; tries < 400; tries++) {
      __sky.fly(0.5);
      camera.position.set(...eye);
      // Solid, within 700 m, and high enough that a 15 degree view of it
      // is mostly sky.
      const ship = kind.ships.find((s) => Math.abs(s.fade) > 0.99 && s.position.distanceTo(camera.position) < 700
        && s.position.y - eye[1] > 0.25 * Math.hypot(s.position.x - eye[0], s.position.z - eye[2]));
      if (!ship) continue;
      camera.fov = 15;
      camera.updateProjectionMatrix();
      camera.lookAt(ship.position.clone().addScaledVector(ship.forward, 0.25 * ship.velocity[2]));
      camera.updateMatrixWorld();
      if (still(ship).own <= 50) continue;
      const saved = snapshot();
      const without = film(saved, false);
      const temporal = film(saved, true);
      const { off, others, own } = still(ship);
      if (own <= 50) continue;
      // This fighter's own pixels (the frame against the frame without it),
      // and around them, where it is drawn in each.
      const near = __sky.shipMask(off, others);
      const b = __sky.shipPixelsNear(off, others, near);
      const a = __sky.shipPixelsNear(temporal, others, near), c = __sky.shipPixelsNear(without, others, near);
      // Its motion as drawn for the upscaler (this fighter's alone) against
      // its motion on screen over the same frame, in UV units.
      const index = kind.ships.indexOf(ship);
      __bf.settings.set('upscaler', 'temporal');
      const was = ship.position.clone().project(camera);
      flyers.update(1 / 60, camera);
      for (const k of flyers.classes) for (const m of k.motions) m.visible = k === kind;
      kind.motion.shown.array.fill(0);
      kind.motion.shown.array[index] = 1;
      kind.motion.shown.needsUpdate = true;
      __bf.graphics.render(25 / 60);
      const is = ship.position.clone().project(camera);
      const expected = [(is.x - was.x) * 0.5, (is.y - was.y) * 0.5];
      // The velocity target is half float: written out as bytes, 0.5 + 25 x
      // the motion, where the motion pass drew a moving body (alpha 0.5).
      const pass = fullScreenPass(`
        uniform highp sampler2D map;
        void main() {
          vec4 v = texelFetch(map, ivec2(gl_FragCoord.xy), 0);
          gl_FragColor = vec4(clamp(0.5 + 25.0 * v.rg, 0.0, 1.0), v.a, 1.0);
        }`, { map: { value: __bf.graphics.upscaler.velocity.texture } });
      __bf.renderer.setRenderTarget(null);
      __bf.renderer.autoClear = false;
      __bf.renderer.render(pass, __bf.graphics.passCamera);
      __bf.renderer.autoClear = true;
      pass.material.dispose();
      const moved = read();
      let n = 0, mx = 0, my = 0;
      for (let i = 0; i < moved.w * moved.h; i++) {
        const k = i * 4;
        if (Math.abs(moved.pixels[k + 2] - 128) > 26) continue;
        n++;
        mx += (moved.pixels[k] / 255 - 0.5) / 25;
        my += (moved.pixels[k + 1] / 255 - 0.5) / 25;
      }
      flyers.update(0, camera);
      const drawn = n ? [mx / n, my / n] : null;
      return { temporal: temporal.url, without: without.url, off: off.url, a, b, c, distance: ship.position.distanceTo(camera.position), tries, motion: { pixels: n, drawn, expected } };
    }
    return null;
  }, [eye]);
  assert.ok(result, 'a droid fighter in reach');
  save(`${OUT}/geonosis-motion-temporal.png`, result.temporal);
  save(`${OUT}/geonosis-motion-without.png`, result.without);
  save(`${OUT}/geonosis-motion-off.png`, result.off);
  const apart = (p) => Math.hypot(p.x - result.b.x, p.y - result.b.y);
  const lag = apart(result.a), stale = apart(result.c);
  console.log(`geonosis: droid fighter at ${result.distance.toFixed(0)} m, ${result.b.changed} px without the upscaler; ` +
    `temporal ${result.a.changed} px, ${lag.toFixed(2)} px off; without their motion ${result.c.changed} px, ${stale.toFixed(2)} px off`);
  const { pixels, drawn, expected } = result.motion;
  console.log(`geonosis: its motion drawn on ${pixels} px, ${drawn?.map((v) => v.toFixed(5)).join(', ')} UV a frame; on screen ${expected.map((v) => v.toFixed(5)).join(', ')}`);
  assert.ok(result.a.changed > 50 && result.b.changed > 50, 'the fighter shows');
  assert.ok(pixels > 50 && drawn, 'the fighter draws its motion');
  const error = Math.hypot(drawn[0] - expected[0], drawn[1] - expected[1]), size = Math.hypot(...expected);
  assert.ok(size > 0.002 && error < 0.15 * size, 'its motion is its motion on screen');
  assert.ok(lag < 4, 'drawn where it is under the temporal upscaler');
}
