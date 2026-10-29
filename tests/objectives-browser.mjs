// The destructible objectives on the page (docs/objectives.md), over the
// world worker: a Techno Union ship on Geonosis standing, dying and gone
// (its hulk left), the map icons, and Hoth's shield generator becoming its
// wreck. Screenshots go to OUT (default /tmp/bfe-objectives-shots).
//
//   PLAYWRIGHT_MODULE=... BASE=http://127.0.0.1:18471 node tests/objectives-browser.mjs
import fs from 'node:fs';
import assert from 'node:assert/strict';
import { launchBrowser, openBattle } from './battle-page.mjs';

const BASE = process.env.BASE || 'http://127.0.0.1:18449';
const OUT = process.env.OUT || '/tmp/bfe-objectives-shots';
fs.mkdirSync(OUT, { recursive: true });
const browser = await launchBrowser();
const shot = (battle, name) => battle.page.screenshot({ path: `${OUT}/${name}.png` });

// Stand the player `back` metres from a post, looking at it: of eight
// directions, the first from which its middle is in clear view.
async function face(battle, name, back, up = 4) {
  await battle.world.evaluate(([name, back, up]) => {
    const g = self.__bf, T = g.THREE;
    const b = g.live.objectives.find((o) => o.desc.post === name);
    const p = b.position;
    const middle = b.center(new T.Vector3());
    const me = self.__server.actor.unit;
    const hit = { point: new T.Vector3(), normal: new T.Vector3() };
    let best = null;
    for (let k = 0; k < 16 && !best; k++) {
      const angle = (k / 16) * Math.PI * 2;
      const x = p.x + Math.sin(angle) * back, z = p.z + Math.cos(angle) * back;
      me.soldier.spawn(x, p.y + 30, z, 0);
      const eye = me.soldier.position.clone();
      eye.y += up;
      const to = middle.clone().sub(eye), span = to.length();
      const blocked = g.collision.cast(eye, to.divideScalar(span), span, hit, b.bodies.built);
      if (!blocked) best = [x, z];
    }
    const [x, z] = best || [p.x + back, p.z];
    // The aim is (sin yaw, cos yaw) on x and z (src/combat.js).
    me.soldier.spawn(x, p.y + 30, z, Math.atan2(p.x - x, p.z - z));
    me.presentation?.reset?.();
  }, [name, back, up]);
  await battle.frames(90, 1 / 60, true);
}

async function destroy(battle, name) {
  await battle.world.evaluate((name) => {
    const b = self.__bf.live.objectives.find((o) => o.desc.post === name);
    b.damage(b.health + 1, null);
  }, name);
}

const state = (battle, name) => battle.page.evaluate((name) => {
  const b = __bf.live.objectives.find((o) => o.desc.post === name);
  const post = __bf.posts.posts.find((p) => p.name === name);
  return { state: b.state, dying: b.dying, visible: b.looks.built.root.visible, active: post.active, health: b.health };
}, name);

{
  const battle = await openBattle(browser, { base: BASE, query: 'map=geo1&side=rep&unit=rep_inf_clone_trooper', viewport: { width: 960, height: 540 } });
  const objectives = await battle.page.evaluate(() => __bf.live.objectives.map((o) => [o.desc.post, o.state, o.looks.built?.root.visible]));
  console.log('geo1 objectives on the page', JSON.stringify(objectives));
  assert.equal(objectives.length, 4);
  await face(battle, 'cp7', 70, 10);
  await shot(battle, 'geo1-cp7-standing');
  await battle.page.keyboard.press('m');
  await battle.frames(10, 1 / 60, true);
  await shot(battle, 'geo1-map');
  await battle.page.keyboard.press('m');
  await battle.frames(10, 1 / 60, true);
  await destroy(battle, 'cp7');
  await battle.frames(150, 1 / 60, true);
  const dying = await state(battle, 'cp7');
  console.log('cp7 2.5 s after', JSON.stringify(dying));
  assert.equal(dying.state, 'destroyed');
  assert.ok(dying.dying > 0 && dying.visible, 'the ship plays its death');
  assert.equal(dying.active, false);
  await shot(battle, 'geo1-cp7-dying');
  await battle.frames(360, 1 / 60, true);
  const gone = await state(battle, 'cp7');
  console.log('cp7 8.5 s after', JSON.stringify(gone));
  assert.equal(gone.visible, false);
  await shot(battle, 'geo1-cp7-gone');
  console.log('geo1 errors', battle.errors.slice(0, 5));
  await battle.close();
}
{
  const battle = await openBattle(browser, { base: BASE, query: 'map=hot1&side=imp&unit=imp_inf_storm_troopersnow', viewport: { width: 960, height: 540 } });
  await face(battle, 'shield', 45, 6);
  await shot(battle, 'hot1-shield-standing');
  await destroy(battle, 'shield');
  await battle.frames(120, 1 / 60, true);
  const after = await state(battle, 'shield');
  console.log('shield 2 s after', JSON.stringify(after));
  assert.equal(after.state, 'destroyed');
  assert.equal(after.active, false);
  await shot(battle, 'hot1-shield-wreck');
  console.log('hot1 errors', battle.errors.slice(0, 5));
  await battle.close();
}
await browser.close();
