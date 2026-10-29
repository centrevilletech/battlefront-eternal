// The ships crossing the sky (src/sky-flyers.js) on the battles that have
// them, drawn in the far scene as the page draws them.
//
// With Vite running and maps prepared since maps generation 15 (set
// PLAYWRIGHT_MODULE if Playwright is elsewhere):
//   node tests/sky-flyers-browser.mjs
// Env: GAME_URL (http://127.0.0.1:18449), SKY_FLYERS_OUTPUT (frames, default
// /tmp/bf-sky-flyers), SKY_FLYERS_ONLY (comma list of battles below),
// SKY_FLYERS_SIZE (960x540).
//
// For each battle, in Classic (Graphics > Ocean and sky; the Enhanced sky
// has tests/sky-flyers-enhanced-browser.mjs): the page flies the classes the
// original does, one instanced draw per model part in the far scene with
// the classes' NumObjects ships; they fly and fade in; then
// the camera stands at one of the battle's camera shots or command posts (or
// over them) and looks at the nearest ship inside the far scene's reach (at
// the original view distance, or 2x when none comes inside it in two
// minutes, as on the maps whose ships keep far out), from whichever eye
// shows most of the ships. That frame must differ from the same frame with
// the ships hidden, and where ships are. Both are saved.
import { mkdirSync, writeFileSync } from 'node:fs';
import assert from 'node:assert/strict';
import { launchBrowser, openBattle, LOW_SETTINGS } from './battle-page.mjs';

const URL = process.env.GAME_URL || 'http://127.0.0.1:18449';
const OUT = process.env.SKY_FLYERS_OUTPUT || '/tmp/bf-sky-flyers';
const ONLY = (process.env.SKY_FLYERS_ONLY || '').split(',').filter(Boolean);
const [WIDTH, HEIGHT] = (process.env.SKY_FLYERS_SIZE || '960x540').split('x').map(Number);
mkdirSync(OUT, { recursive: true });

const BATTLES = [
  { name: 'geonosis', query: 'map=geo1&side=rep&unit=rep_inf_clone_trooper',
    counts: { rep_fly_assault_dome: 2, rep_fly_gunship_dome: 20, cis_fly_droidfighter_dome: 20, cis_fly_techounion_dome: 8 } },
  { name: 'dune-sea-gcw', query: 'map=tat1&side=imp&unit=imp_inf_storm_trooper', counts: { imp_fly_destroyer_dome: 2 } },
  { name: 'dune-sea-cw', query: 'map=tat1&side=rep&unit=rep_inf_clone_trooper', counts: { rep_fly_assault_dome: 2 } },
  { name: 'naboo-plains-gcw', query: 'map=nab1&side=imp&unit=imp_inf_storm_trooper', counts: { imp_fly_destroyer_dome: 1 } },
  { name: 'naboo-plains-cw', query: 'map=nab1&side=rep&unit=rep_inf_clone_trooper', counts: { cis_fly_fedlander_dome: 1 } },
  { name: 'mos-eisley-gcw', query: 'map=tat2&side=imp&unit=imp_inf_storm_trooper', counts: { imp_fly_destroyer_dome: 2 } },
  { name: 'cloud-city-cw', query: 'map=bes2&side=rep&unit=rep_inf_clone_trooper', counts: { rep_fly_assault_dome: 2 } },
  { name: 'cloud-city-gcw', query: 'map=bes2&side=imp&unit=imp_inf_storm_trooper', counts: {} },
].filter((battle) => !ONLY.length || ONLY.includes(battle.name));

// On the page: the classes built, and a view of a ship in reach.
function pageHelpers() {
  window.__sky = {
    built() {
      const flyers = __bf.skyFlyers;
      if (!flyers) return {};
      // The classes flying in this mode (Classic here: the setting is
      // LOW_SETTINGS's); in Classic every ship is in the one draw a part.
      return Object.fromEntries(flyers.classes.filter((kind) => flyers.active(kind)).map(({ def, ships, draws, fades }) => [def.model, {
        ships: ships.length, draws: draws.length, instanced: draws.every((m) => m.isInstancedMesh && m.count === ships.length),
        inFarScene: [...draws, ...fades].every((m) => m.parent?.parent === __bf.graphics.farScene),
      }]));
    },
    // Fly the ships on (the world stays where it is), `seconds` in 1/30 s.
    fly(seconds) {
      for (let t = 0; t < seconds; t += 1 / 30) __bf.skyFlyers.update(1 / 30, __bf.camera);
    },
    // Where the camera may stand: the battle's camera shots, its posts, and
    // 40 m over the highest post at the posts' middle (Cloud City's posts
    // are indoors).
    eyes() {
      const shots = (__bf.spawnPort?.cameraShots?.() || []).map((s) => s.position);
      const posts = (__bf.posts?.posts || []).map((p) => p.position).filter(Boolean);
      const eyes = [...shots, ...posts].map((p) => [p.x ?? p[0], (p.y ?? p[1]) + 2, p.z ?? p[2]]);
      if (posts.length) {
        const mid = (k) => posts.reduce((sum, p) => sum + p[k], 0) / posts.length;
        eyes.push([mid('x'), Math.max(...posts.map((p) => p.y)) + 40, mid('z')]);
      }
      return eyes;
    },
    // For each eye, the nearest ship drawn at 0.6 alpha or more (fading in
    // or out) inside the far scene's reach and well above it; the nearest
    // few of those.
    pick(eyes) {
      const reach = __bf.graphics.farCamera.far * 0.92;
      const found = [];
      for (const eye of eyes) {
        let best = null;
        for (const { def, ships } of __bf.skyFlyers.classes.filter((kind) => __bf.skyFlyers.active(kind))) {
          for (const ship of ships) {
            const d = Math.hypot(ship.position.x - eye[0], ship.position.y - eye[1], ship.position.z - eye[2]);
            if (Math.abs(ship.fade) >= 0.6 && d < reach && ship.position.y > eye[1] + 15 && (!best || d < best.distance)) {
              best = { model: def.model, eye, at: ship.position.toArray(), distance: d };
            }
          }
        }
        if (best) found.push(best);
      }
      return found.sort((a, b) => a.distance - b.distance).slice(0, 16);
    },
    // The frame from `eye` looking a little under `at` (so the ground
    // shows), with the ships shown and hidden, read back as pixels.
    film(eye, at, fov) {
      const { camera, graphics, renderer, skyFlyers } = __bf;
      camera.fov = fov;
      camera.updateProjectionMatrix();
      camera.position.set(...eye);
      camera.lookAt(at[0], at[1] - 0.25 * Math.hypot(at[0] - eye[0], at[2] - eye[2]) * Math.tan(fov * Math.PI / 360), at[2]);
      camera.updateMatrixWorld();
      skyFlyers.update(0, camera);
      const gl = renderer.getContext();
      const w = gl.drawingBufferWidth, h = gl.drawingBufferHeight;
      const grab = () => {
        graphics.render(1);
        const pixels = new Uint8Array(w * h * 4);
        gl.readPixels(0, 0, w, h, gl.RGBA, gl.UNSIGNED_BYTE, pixels);
        return { pixels, url: renderer.domElement.toDataURL('image/png') };
      };
      const shown = grab();
      skyFlyers.group.visible = false;
      const hidden = grab();
      skyFlyers.group.visible = true;
      let changed = 0, minX = w, minY = h, maxX = -1, maxY = -1;
      for (let i = 0; i < w * h; i++) {
        const k = i * 4;
        if (Math.abs(shown.pixels[k] - hidden.pixels[k]) + Math.abs(shown.pixels[k + 1] - hidden.pixels[k + 1]) + Math.abs(shown.pixels[k + 2] - hidden.pixels[k + 2]) > 24) {
          changed++;
          const x = i % w, y = h - 1 - Math.floor(i / w);
          minX = Math.min(minX, x); maxX = Math.max(maxX, x); minY = Math.min(minY, y); maxY = Math.max(maxY, y);
        }
      }
      // Where every ship in front of the eye and inside the reach lands.
      // (The eye's own far plane is the near scene's: only x and y count.)
      const forward = camera.getWorldDirection(camera.position.clone());
      const toScreen = (position) => {
        const p = position.clone().project(camera);
        return [(p.x + 1) / 2 * w, (1 - p.y) / 2 * h, position.clone().sub(camera.position).dot(forward)];
      };
      const ships = skyFlyers.classes.filter((kind) => skyFlyers.active(kind)).flatMap(({ ships }) => ships)
        .filter((ship) => ship.fade !== 0 && ship.position.distanceTo(camera.position) < graphics.farCamera.far)
        .map((ship) => toScreen(ship.position)).filter(([x, y, ahead]) => ahead > 0 && x >= 0 && x < w && y >= 0 && y < h);
      return {
        shown: shown.url, hidden: hidden.url, changed, box: changed ? [minX, minY, maxX, maxY] : null,
        target: toScreen(camera.position.clone().set(...at)), ships, size: [w, h],
        far: graphics.farCamera.far, calls: graphics.frameInfo?.calls ?? null,
      };
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
        settings: { ...LOW_SETTINGS, upscaler: 'off', viewDistance: 'original' }, timeout: 600000 });
      const { page } = opened;
      await page.evaluate(pageHelpers);
      const built = await page.evaluate(() => __sky.built());
      assert.deepEqual(Object.fromEntries(Object.entries(built).map(([model, b]) => [model, b.ships])), battle.counts, `${battle.name}: classes`);
      for (const [model, b] of Object.entries(built)) assert.ok(b.instanced && b.inFarScene && b.draws >= 1, `${battle.name}: ${model} drawn instanced in the far scene`);
      if (!Object.keys(battle.counts).length) {
        console.log(`${battle.name}: no flyers, as expected (${Math.round((Date.now() - started) / 1000)} s)`);
        continue;
      }
      // The ships fade in and fly on until one is in reach; failing that,
      // at the 2x view distance.
      const eyes = await page.evaluate(() => __sky.eyes());
      assert.ok(eyes.length, `${battle.name}: somewhere to stand`);
      await page.evaluate(() => __sky.fly(6));
      let picks = [], distance = 'original';
      const look = () => page.evaluate(([eyes]) => { const p = __sky.pick(eyes); if (!p.length) __sky.fly(2); return p; }, [eyes]);
      for (let t = 0; t < 120 && !picks.length; t += 2) picks = await look();
      if (!picks.length) {
        distance = 'far';
        await page.evaluate(() => { __bf.settings.set('viewDistance', 'far'); });
        for (let t = 0; t < 120 && !picks.length; t += 2) picks = await look();
      }
      assert.ok(picks.length, `${battle.name}: a ship comes inside the far scene's reach`);
      // The clearest of the nearest views: the most of the ships showing
      // (a post under a roof sees little of them).
      let pick = null, close = null;
      for (const candidate of picks) {
        const view = await page.evaluate(([eye, at]) => __sky.film(eye, at, 40), [candidate.eye, candidate.at]);
        if (!close || view.changed > close.changed) { pick = candidate; close = view; }
      }
      const wide = await page.evaluate(([eye, at]) => __sky.film(eye, at, 75), [pick.eye, pick.at]);
      save(`${OUT}/${battle.name}-ship.png`, close.shown);
      save(`${OUT}/${battle.name}-ship-hidden.png`, close.hidden);
      save(`${OUT}/${battle.name}-sky.png`, wide.shown);
      save(`${OUT}/${battle.name}-sky-hidden.png`, wide.hidden);
      const [x, y] = close.target, box = close.box;
      console.log(`${battle.name}: ${pick.model} at ${pick.distance.toFixed(0)} m, view distance ${distance} (far plane ${close.far} m); ` +
        `${close.changed} pixels from the ships, box ${JSON.stringify(box)}, aimed at ${x.toFixed(0)},${y.toFixed(0)}, ${close.ships.length} ships in view; ` +
        `wide view ${wide.changed} pixels (${Math.round((Date.now() - started) / 1000)} s)`);
      assert.ok(close.changed > 20, `${battle.name}: the ships show`);
      // What changed is where ships are (the one aimed at may stand behind
      // a building while others show).
      const inside = ([sx, sy]) => sx >= box[0] - 40 && sx <= box[2] + 40 && sy >= box[1] - 40 && sy <= box[3] + 40;
      assert.ok(close.ships.some(inside), `${battle.name}: where the ships are`);
      const errors = opened.errors.filter((e) => /sky|flyer|shader|WebGL|program/i.test(e));
      assert.deepEqual(errors, [], `${battle.name}: errors`);
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
console.log(`all ${BATTLES.length} battles pass; frames in ${OUT}`);
