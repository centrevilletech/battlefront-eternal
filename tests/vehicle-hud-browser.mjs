// Requires Vite plus extracted Hoth (hot1), Tatooine: Dune Sea (tat1) and
// Bespin: Platforms (bes1). He boards every kind of hull the maps have (a
// creature, walkers, hovers with open and closed seats, craft) and leaves it
// every way there is: E on the ground, E in flight (the bail out), killed
// on the seat (the pause menu's RESPAWN), the hull destroyed under him,
// shot off an open seat, a seat switch and E from there; then he respawns.
// Seated, the HUD the page draws is the hull's (its icon in the health
// group, the seat's weapons, a landed craft's takeoff prompt for its pilot
// alone). After each way out it is his own again: no seat on the page, the
// HUD's quads carry his soldier's health icon and weapon and none of the
// hull's, no prompt of a craft's, and nothing else drawn over the game's
// canvas (the old flight HUD's canvas outlived every seat). The world (in
// its worker, tests/battle-page.mjs) seats him; the page's presses take him
// out. Frames go to VEHICLE_HUD_ARTIFACTS. PLAYWRIGHT_MODULE can point at
// an external Playwright install.
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import { launchBrowser, openBattle } from './battle-page.mjs';
const base = process.env.VEHICLE_TEST_URL || 'http://127.0.0.1:18449';
const out = process.env.VEHICLE_HUD_ARTIFACTS || '/tmp/bf-vehicle-hud';
await fs.mkdir(out, { recursive: true });

// Each map's side and the hulls checked there, with the ways he leaves.
const ALL_MAPS = [
  { map: 'hot1', side: 'all', hulls: { all_walk_tauntaun: ['E', 'killed', 'shot'], all_fly_snowspeeder: ['E', 'eject', 'seat', 'destroyed'], imp_walk_atst_snow: ['E', 'seat'], imp_walk_atat: ['E', 'destroyed'] } },
  { map: 'tat1', side: 'all', hulls: { all_fly_xwing: ['E', 'eject', 'killed', 'destroyed'], imp_hover_speederbike: ['E', 'shot'], tat_hover_skiff: ['seat', 'shot'], all_hover_combatspeeder: ['E', 'seat', 'destroyed'] } },
  { map: 'bes1', side: 'rep', hulls: { rep_fly_gunship: ['E', 'eject', 'seat', 'destroyed'], rep_fly_jedifighter: ['E', 'eject', 'killed'] } },
];
// VEHICLE_HUD_MAPS=hot1,tat1 checks only those.
const MAPS = ALL_MAPS.filter(({ map }) => !process.env.VEHICLE_HUD_MAPS || process.env.VEHICLE_HUD_MAPS.split(',').includes(map));

// What the page shows this frame: its seat, the HUD's state, and what one
// drawn frame of the HUD puts on screen (the textures of its quads and its
// text), with the hull's and the soldier's own icons for comparison.
const shown = async (page, name) => page.evaluate(async (name) => {
  const b = __bf, hud = b.hud, quads = [], texts = [];
  const { quad, quadCorners } = hud.batch, draw = hud.fonts.draw;
  hud.batch.quad = function (...args) { quads.push(args[5]); return quad.apply(this, args); };
  hud.batch.quadCorners = function (...args) { quads.push(args[5]); return quadCorners.apply(this, args); };
  hud.fonts.draw = function (...args) { texts.push(String(args[4])); return draw.apply(this, args); };
  try { await b.lockstep(1 / 60, true); } finally { Object.assign(hud.batch, { quad, quadCorners }); hud.fonts.draw = draw; }
  const used = (texture) => Boolean(texture) && quads.includes(texture);
  const v = b.vehicles.vehicles.find((x) => x.name === name);
  const icons = (weapons) => weapons.map((w) => w?.def?.IconTexture).filter(Boolean);
  const hullIcons = [...new Set([v.def.settings.HealthTexture, ...v.seats.flatMap((seat) => icons(seat.weapons.map((w) => w.state)))])]
    .filter((icon) => !icons(b.combat.arsenal.weapons).includes(icon));
  const ownWeapon = b.combat.arsenal.current(0);
  // Anything drawn over the game's canvas: another canvas showing.
  const canvases = [...document.querySelectorAll('canvas')].filter((c) => c !== b.renderer.domElement && getComputedStyle(c).display !== 'none' && c.width && c.height);
  const overlays = canvases.filter((c) => { const d = c.getContext('2d')?.getImageData(0, 0, c.width, c.height).data; if (!d) return true; for (let i = 3; i < d.length; i += 4) if (d[i]) return true; return false; }).length;
  return {
    driver: b.driver.vehicle?.name ?? null, seat: b.playerUnit.seat ? b.playerUnit.seat.index : null, mounted: b.combat.mounted, dead: b.combat.dead,
    landedPilot: Boolean(b.driver.vehicle?.flying && b.driver.vehicle.seatIndex === 0 && b.driver.vehicle.flightState === 'Landed'),
    hullHealthDrawn: !v.def.settings.HealthTexture || used(hud.texture(v.def.settings.HealthTexture)),
    hudVehicle: hud.vehicle?.name ?? null, ownArsenal: hud.arsenal === b.combat.arsenal,
    hullDrawn: hullIcons.filter((icon) => used(hud.texture(icon))),
    ownHealthDrawn: used(hud.texture('hud_health')),
    ownWeaponDrawn: !ownWeapon?.def.IconTexture || used(hud.texture(ownWeapon.def.IconTexture)),
    reticle: hud.reticle ? hud.reticle.weapon === ownWeapon : null,
    prompts: texts.filter((t) => /TAKE ?OFF|TO LAND/i.test(t)),
    texts: texts.filter((t) => /^[A-Z][A-Z .:/-]+$/.test(t)).slice(0, 12),
    overlays,
  };
}, name);

const browser = await launchBrowser();
const failures = [];
try {
  for (const { map, side, hulls } of MAPS) {
    const battle = await openBattle(browser, { base, query: `map=${map}&side=${side}&ai=off&shadows=off&loading=0`, viewport: { width: 960, height: 540 } });
    const { page, world, errors } = battle;
    let shot = 0;
    const capture = (label) => page.screenshot({ path: `${out}/${map}-${String(++shot).padStart(2, '0')}-${label}.png` });
    // Alive on his feet, at the first post.
    const onFoot = async () => {
      await world.evaluate(() => { const b = self.__bf; if (b.combat.dead || !b.playerUnit.alive || b.playerUnit.removed || b.driver.vehicle) b.respawn(0); b.combat.reset(); });
      await battle.frames(5);
    };
    // Into the first free hull of the class; seat 0.
    const board = async (name) => {
      await onFoot();
      const ok = await world.evaluate((name) => {
        const b = self.__bf, v = b.vehicles.vehicles.find((x) => x.name === name && x.alive && !x.driver && x.seats.every((seat) => !seat.lease));
        if (!v) return false;
        self.__hull = v;
        b.soldier.position.copy(v.position).add(new b.THREE.Vector3(3, 0, 0));
        return b.driver.enter(v);
      }, name);
      assert.ok(ok, `${map}: he boards a ${name}`);
      await battle.frames(10);
    };
    // The page shows the seat: the hull's HUD.
    const seated = async (name, label) => {
      const s = await shown(page, name);
      console.log(JSON.stringify({ map, name, label, ...s }));
      assert.equal(s.driver, name, `${map} ${name} ${label}: the page seats him`);
      assert.equal(s.hudVehicle, name, `${map} ${name} ${label}: the HUD shows the hull`);
      assert.ok(!s.ownArsenal, `${map} ${name} ${label}: the HUD shows the seat's weapons`);
      assert.equal(s.overlays, 0, `${map} ${name} ${label}: nothing but the game's canvas draws`);
      assert.ok(s.hullHealthDrawn, `${map} ${name} ${label}: the health group shows the hull's icon`);
      if (s.landedPilot) assert.deepEqual(s.prompts, ['SPACE TO TAKE OFF'], `${map} ${name} ${label}: a landed craft's pilot is told how to take off`);
      return s;
    };
    // Off the seat, alive or dead, the HUD is his own.
    const back = async (name, way, { dead = false } = {}) => {
      const s = await shown(page, name);
      console.log(JSON.stringify({ map, name, way, ...s }));
      const check = (ok, what) => { if (!ok) failures.push(`${map} ${name} after ${way}: ${what}`); };
      check(s.driver === null && s.seat === null && !s.mounted, `the page still seats him (${s.driver}, seat ${s.seat})`);
      check(s.hudVehicle === null, `the HUD still shows ${s.hudVehicle}`);
      check(s.ownArsenal, 'the HUD shows the seat\'s weapons');
      check(s.hullDrawn.length === 0, `the hull's icons are drawn: ${s.hullDrawn}`);
      check(s.prompts.length === 0, `a craft's prompt is drawn: ${s.prompts}`);
      check(s.overlays === 0, `${s.overlays} canvas(es) drawn over the game's`);
      check(s.dead === dead, `dead ${s.dead}, expected ${dead}`);
      if (!dead) {
        check(s.ownHealthDrawn, 'his health icon is not drawn');
        check(s.ownWeaponDrawn, 'his weapon\'s icon is not drawn');
        check(s.reticle === true, 'the reticle is not his weapon\'s');
      }
      await capture(`${name}-${way}`);
    };
    const press = async (action, frames) => { await page.evaluate((action) => { __bf.pending[action] = true; }, action); await battle.frames(frames); };
    const respawn = async (name, way) => { await onFoot(); await back(name, `${way}, respawned`); };
    for (const [name, ways] of Object.entries(hulls)) {
      for (const way of ways) {
        await board(name);
        await seated(name, 'boarded');
        if (way === 'E') {
          await press('use', 30);
          await back(name, 'E');
        } else if (way === 'eject') {
          await press('takeoff', 30);
          let state = null;
          // A gunship lifts off over a few seconds.
          for (let frames = 30; frames < 900 && state !== 'Flying'; frames += 30) {
            await battle.frames(30);
            state = await world.evaluate(() => self.__hull.flightState);
          }
          assert.equal(state, 'Flying', `${map} ${name}: it takes off`);
          await seated(name, 'flying');
          await capture(`${name}-flying`);
          await press('use', 20);
          await back(name, 'eject');
        } else if (way === 'seat') {
          await press('nextPrimary', 20);
          const s = await seated(name, 'next seat');
          assert.notEqual(await page.evaluate(() => __bf.driver.vehicle.seatIndex), 0, `${map} ${name}: he moves to another seat`);
          assert.equal(s.prompts.length, 0, `${map} ${name}: a gunner sees no pilot's prompt`);
          await capture(`${name}-seat2`);
          await press('use', 30);
          await back(name, 'E from the next seat');
        } else if (way === 'destroyed') {
          await world.evaluate(() => self.__hull.damage(Infinity));
          await battle.frames(30);
          await back(name, 'the hull destroyed', { dead: true });
          await respawn(name, 'the hull destroyed');
        } else if (way === 'killed') {
          await page.evaluate(() => __bf.spawnPort.suicide());
          await battle.frames(30);
          await back(name, 'killed on the seat', { dead: true });
          await respawn(name, 'killed on the seat');
        } else if (way === 'shot') {
          // A lethal hit on him, not the hull (an enemy's where one is
          // about): an open seat's rider falls off it dead.
          await world.evaluate(() => { const b = self.__bf, enemy = b.units.all().find((u) => u.alive && u.soldier && b.units.relation(u.team, b.playerUnit.team) < 0) ?? null; b.combat.damage(Infinity, Infinity, null, enemy); });
          await battle.frames(30);
          await back(name, 'shot off', { dead: true });
          await respawn(name, 'shot off');
        }
      }
    }
    assert.deepEqual(errors, [], `${map}: no page errors`);
    await battle.close();
  }
} finally {
  await browser.close();
}
console.log(JSON.stringify({ failures }, null, 1));
assert.deepEqual(failures, [], 'the on-foot HUD is back after every way of leaving');
