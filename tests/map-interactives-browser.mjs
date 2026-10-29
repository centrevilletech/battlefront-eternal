// Requires Vite and prepared maps (bes2 at maps generation 9 or later; yav2
// at generation 12 for its placed power-up). Each map's interactive object
// as a player meets it, in the world worker and on the page:
// - Bespin: Cloud City's carbon-freeze forklift (bes2_carbon_forklift):
//   the player steps onto hp_activate and its activate clip plays
//   (AnimationTrigger "activate hp_activate 0.5", src/props.js
//   AnimatedProp); frames before and during it.
// - Yavin 4: Arena's placed health power-up (src/placed-items.js): it lies
//   where the map placed it on the page too, and the player takes it.
// Frames go to MAP_INTERACTIVES_ARTIFACTS when it is set.
import fs from 'node:fs/promises';
import { launchBrowser, openBattle, LOW_SETTINGS } from './battle-page.mjs';
const base = process.env.MAP_INTERACTIVES_URL || 'http://127.0.0.1:18449';
const artifacts = process.env.MAP_INTERACTIVES_ARTIFACTS;
const check = (ok, message, detail) => { if (!ok) throw new Error(`${message}: ${JSON.stringify(detail)}`); };
const save = async (name, url) => {
  if (!artifacts || !url) return;
  await fs.mkdir(artifacts, { recursive: true });
  await fs.writeFile(`${artifacts}/${name}.png`, Buffer.from(url.split(',')[1], 'base64'));
};
// The page's camera at `eye` looking at `at`, drawn once.
const shot = (page, eye, at) => page.evaluate(async ({ eye, at }) => {
  const b = __bf;
  await b.lockstep(0, true);
  const saved = { p: b.camera.position.clone(), q: b.camera.quaternion.clone() };
  b.camera.position.fromArray(eye); b.camera.lookAt(at[0], at[1], at[2]); b.camera.updateMatrixWorld(true);
  b.graphics.render(0);
  const url = b.renderer.domElement.toDataURL();
  b.camera.position.copy(saved.p); b.camera.quaternion.copy(saved.q);
  return url;
}, { eye, at });

// Until the player is on the field (his first spawn), up to `limit` frames.
async function untilAlive(battle, limit = 1200) {
  for (let i = 0; i < limit; i += 30) {
    if (await battle.world.evaluate(() => self.__server.actor.unit.alive)) return true;
    await battle.frames(30);
  }
  return false;
}

const browser = await launchBrowser();
try {
  // Bespin: Cloud City's carbon-freeze forklift.
  {
    const battle = await openBattle(browser, { base, query: 'map=bes2&side=all&unit=all_inf_soldierurban&seed=1',
      viewport: { width: 800, height: 500 }, settings: LOW_SETTINGS });
    const { page, world, errors } = battle;
    const fork = await world.evaluate(() => {
      const prop = self.__bf.live.animated.find((p) => p.desc.class === 'bes2_carbon_forklift');
      return prop && { point: prop.trigger.point.toArray(), at: prop.position.toArray(), slot: prop.slot, names: prop.names, radius: prop.trigger.radius };
    });
    check(fork && fork.names.includes('activate'), 'the forklift is live with its activate slot', fork);
    check(await untilAlive(battle), 'the player spawned', {});
    const [x, y, z] = fork.point, [fx, fy, fz] = fork.at;
    const eye = [fx + 9, fy + 7, fz + 9], look = [fx, fy + 4, fz];
    await save('bes2-forklift-idle', await shot(page, eye, look));
    // Idle while no one is at hp_activate.
    await battle.frames(60);
    const idle = await world.evaluate(() => self.__bf.live.animated.find((p) => p.desc.class === 'bes2_carbon_forklift').slot);
    check(idle === 0, 'with no one in the pit, it idles', { idle });
    // The player drops into the carbon-freeze pit beside the claws' column
    // (1 m round, hanging to 5 m over the floor), over hp_activate: the
    // pit's death region (deathregion) kills him on the way down, his body
    // lies at the bottom, and a body there (living or dead) sets it off.
    await world.evaluate(([x, y, z]) => self.__server.actor.unit.soldier.spawn(x + 1.3, y + 3, z, 0), fork.point);
    let started = -1, died = -1;
    for (let frame = 0; frame < 240 && started < 0; frame += 5) {
      await battle.frames(5);
      const state = await world.evaluate(() => ({ slot: self.__bf.live.animated.find((p) => p.desc.class === 'bes2_carbon_forklift').slot,
        alive: self.__server.actor.unit.alive, removed: self.__server.actor.unit.removed }));
      if (!state.alive && died < 0) died = frame;
      if (state.slot === fork.names.indexOf('activate')) started = frame;
    }
    const body = await world.evaluate(() => ({ feet: self.__server.actor.unit.position.toArray(), removed: self.__server.actor.unit.removed }));
    check(died >= 0 && started >= died, 'his body at the bottom of the pit sets it off', { died, started, body });
    // It runs its 16.6 s; frames a third and two thirds in.
    await battle.frames(330);
    const mid = await page.evaluate(() => {
      const prop = __bf.live.animated.find((p) => p.desc.class === 'bes2_carbon_forklift');
      const casing = prop.rig.byName.bone_casing, claw = prop.rig.byName.bone_root;
      return { slot: prop.slot, casing: casing?.position.y, claw: claw?.position.y };
    });
    await save('bes2-forklift-activate-1', await shot(page, eye, look));
    check(mid.slot === fork.names.indexOf('activate') && mid.claw < 10, 'the page plays the clip: the claws come down', mid);
    await battle.frames(330);
    const late = await page.evaluate(() => {
      const prop = __bf.live.animated.find((p) => p.desc.class === 'bes2_carbon_forklift');
      return { slot: prop.slot, casing: prop.rig.byName.bone_casing?.position.y };
    });
    await save('bes2-forklift-activate-2', await shot(page, eye, look));
    check(late.casing > 5, 'the casing has risen out of the pit', late);
    console.log('bes2 forklift', JSON.stringify({ trigger: fork.point, died, started, body, mid, late }));
    check(!errors.length, 'no page errors on bes2', errors);
    await battle.close();
  }
  // Yavin 4: Arena's placed health power-up.
  {
    const battle = await openBattle(browser, { base, query: 'map=yav2&side=all&unit=all_inf_soldier&seed=1',
      viewport: { width: 800, height: 500 }, settings: LOW_SETTINGS });
    const { page, world, errors } = battle;
    check(await untilAlive(battle), 'the player spawned', {});
    const placed = await world.evaluate(() => {
      const b = self.__bf, entry = b.data.props?.items?.[0];
      const item = entry && b.items.items.find((i) => i.def === entry.item);
      return entry && { at: entry.matrix.slice(12, 15), position: item?.position.toArray(), gone: item?.gone ?? null };
    });
    check(placed && placed.position && !placed.gone, 'the world spawned the placed power-up', placed);
    const shown = await page.evaluate(() => [...__bf.items.views].map((v) => ({ class: v.def.class, position: v.position.toArray(), mesh: Boolean(v.mesh) })));
    check(shown.some((v) => v.class === 'com_item_powerup_health100' && v.mesh), 'the page draws it from its item record', shown);
    const [x, y, z] = placed.position;
    await save('yav2-placed-health', await shot(page, [x + 2.5, y + 1.6, z + 2.5], [x, y + 0.2, z]));
    await save('yav2-placed-health-wide', await shot(page, [x + 9, y + 5, z + 9], [x, y, z]));
    // Hurt, the player walks onto it and takes it.
    await world.evaluate(([x, y, z]) => { const me = self.__server.actor.unit; me.combat.damage(150); me.soldier.spawn(x, y + 0.5, z, 0); }, placed.position);
    const before = await world.evaluate(() => self.__server.actor.unit.combat.health);
    await battle.frames(20);
    const after = await world.evaluate(() => {
      const b = self.__bf, entry = b.data.props.items[0];
      return { health: self.__server.actor.unit.combat.health, gone: b.items.items.find((i) => i.def === entry.item)?.gone ?? true };
    });
    check(after.gone && after.health > before, 'the player took it', { before, after });
    await battle.frames(5);
    const left = await page.evaluate(() => [...__bf.items.views].filter((v) => v.def.class === 'com_item_powerup_health100').length);
    check(left === 0, 'and the page no longer draws it', { left });
    console.log('yav2 placed power-up', JSON.stringify({ placed, before, after }));
    check(!errors.length, 'no page errors on yav2', errors);
    await battle.close();
  }
  console.log('map interactives: ok');
} finally {
  await browser.close();
}
