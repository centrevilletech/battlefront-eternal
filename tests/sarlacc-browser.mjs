// Requires Vite and the extracted Tatooine: Dune Sea (tat1) with the
// Republic (maps generation 9: the tentacle's soft skin and its grabs).
// The world (in its worker) puts the player at a sarlacc tentacle's
// hotspot and a bot at another's; each tentacle grabs, seizes, carries and
// swallows its man as the original game's animated prop does (src/props.js),
// and the page draws them held where the world has them. A third soldier
// walks into the pit's death region (Sarlac01). Frames of the pit from
// three angles and from the player's own view go to SARLACC_TEST_ARTIFACTS
// with the death camera's style (SARLACC_TEST_MODE: classic or enhanced).
import fs from 'node:fs/promises';
import { launchBrowser, openBattle, LOW_SETTINGS } from './battle-page.mjs';
const base = process.env.SARLACC_TEST_URL || 'http://127.0.0.1:18449';
const artifacts = process.env.SARLACC_TEST_ARTIFACTS;
const mode = process.env.SARLACC_TEST_MODE || 'classic';
const check = (ok, message, detail) => { if (!ok) throw new Error(`${message}: ${JSON.stringify(detail)}`); };
const browser = await launchBrowser();
try {
  const battle = await openBattle(browser, { base, query: 'map=tat1&side=rep&unit=rep_inf_clone_trooper&ai=classic&seed=1',
    viewport: { width: 640, height: 400 }, settings: { ...LOW_SETTINGS, deathCamera: mode } });
  const { page, world, errors } = battle;
  // The tentacles' soft skin: one draw blending three bones a vertex.
  const skin = await page.evaluate(() => {
    const prop = __bf.live.animated[1];
    const mesh = prop.rig.meshes[0], w = mesh?.geometry.attributes.skinWeight;
    let blended = 0;
    for (let i = 0; i < (w?.count || 0); i++) if (w.getY(i) > 0.01) blended++;
    return { meshes: prop.rig.meshes.length, vertices: w?.count || 0, blended };
  });
  check(skin.meshes === 1 && skin.blended > 50, 'the tentacle blends its bones', skin);
  // Each tentacle's hotspot and where its root stands, as the world has them.
  const setup = await world.evaluate(() => {
    const b = self.__bf, props = b.live.animated;
    return props.map((prop) => {
      prop.rig.root.updateMatrixWorld(true);
      const v = new b.THREE.Vector3();
      prop.rig.byName.hp_hotspot.getWorldPosition(v);
      return { root: new b.THREE.Vector3().setFromMatrixPosition(prop.rig.root.matrixWorld).toArray(), hotspot: v.toArray() };
    });
  });
  const pit = await world.evaluate(() => self.__bf.data?.world?.deathRegions?.[0]?.matrix?.slice(12, 15) ?? null)
    ?? [-260.954, 3.714, 92.598];
  // The battle's bots come in over its first seconds.
  for (let i = 0; i < 40; i++) {
    const living = await world.evaluate(() => self.__bf.factory.built.filter((u) => u !== self.__server.actor.unit && u.soldier && !u.walker && u.alive && !u.seat).length);
    if (living >= 2) break;
    await battle.frames(30);
  }
  // The player at tentacle 1's hotspot, a bot at tentacle 3's, another bot
  // in the pit; the battle's other bots stay well away.
  const cast = await world.evaluate(({ setup, pit }) => {
    const b = self.__bf, me = self.__server.actor.unit;
    const bots = b.factory.built.filter((u) => u !== me && u.soldier && !u.walker && u.alive && !u.seat);
    const far = new b.THREE.Vector3(pit[0], pit[1], pit[2]);
    for (const u of bots) if (u.position.distanceTo(far) < 60) u.soldier.spawn(u.position.x + 120, u.position.y + 40, u.position.z, 0);
    // Held still: whatever the AI asks of them, they stand where put.
    const place = (u, [x, y, z]) => {
      u.soldier.spawn(x, y + 1, z, Math.PI); u.presentation?.reset?.();
      if (u !== me) { const tick = u.tick.bind(u); u.tick = (h) => { Object.assign(u.intent, { forward: 0, right: 0, turn: 0, jump: false, buttons: [0, 0] }); tick(h); }; }
    };
    place(me, setup[1].hotspot);
    place(bots[0], setup[3].hotspot);
    place(bots[1], [pit[0] + 0.5, pit[1] + 3, pit[2]]);
    return { me: me.id, bot: bots[0].id, faller: bots[1].id };
  }, { setup, pit });
  await battle.frames(1);
  const look = setup[1].root;
  const shots = [[look[0] + 14, look[1] + 12, look[2] + 14], [look[0] - 16, look[1] + 12, look[2] + 4], [look[0] + 2, look[1] + 22, look[2] - 12]];
  const track = [];
  const seized = {};
  for (let frame = 0; frame < 300; frame++) {
    const draw = artifacts && frame % 10 === 0;
    await battle.frame(1 / 60, false);
    const w = await world.evaluate(({ me, bot, faller }) => {
      const b = self.__bf, at = (id) => b.units.byId(id);
      const state = (u) => ({ p: u.position.toArray().map((n) => +n.toFixed(3)), alive: u.alive, dead: u.dead, removed: u.removed,
        held: u.held ? b.live.animated.indexOf(u.held.prop) : -1, killer: u.combat.lastAttacker ? 1 : 0, life: u.generation });
      return { me: state(at(me)), bot: state(at(bot)), faller: state(at(faller)), slots: b.live.animated.map((p) => p.slot) };
    }, cast);
    const p = await page.evaluate(async ({ draw, shots, look, bot }) => {
      const b = __bf, other = b.units.byId(bot);
      const out = { me: b.actor.root.position.toArray().map((n) => +n.toFixed(3)), held: Boolean(b.playerUnit.held), death: b.deathView.active,
        bot: other?.actor.root.position.toArray().map((n) => +n.toFixed(3)), botHeld: Boolean(other?.held?.frame), slots: b.live.animated.map((p) => p.slot) };
      if (draw) {
        out.images = [];
        // His own view as drawn, then the pit from the fixed angles.
        await b.lockstep(0, true);
        out.images.push(b.renderer.domElement.toDataURL());
        const saved = { p: b.camera.position.clone(), q: b.camera.quaternion.clone() };
        for (const eye of shots) {
          b.camera.position.fromArray(eye); b.camera.lookAt(look[0], look[1] + 6, look[2]); b.camera.updateMatrixWorld(true);
          b.graphics.render(0);
          out.images.push(b.renderer.domElement.toDataURL());
        }
        b.camera.position.copy(saved.p); b.camera.quaternion.copy(saved.q);
      }
      return out;
    }, { draw, shots, look, bot: cast.bot });
    for (const who of ['me', 'bot']) if (w[who].held >= 0 && seized[who] === undefined) seized[who] = frame;
    track.push({ frame, world: w, page: { ...p, images: undefined } });
    if (p.images) {
      await fs.mkdir(artifacts, { recursive: true });
      for (const [i, image] of p.images.entries()) await fs.writeFile(`${artifacts}/sarlacc-${mode}-f${String(frame).padStart(3, '0')}-${i ? `v${i - 1}` : 'own'}.png`, Buffer.from(image.split(',')[1], 'base64'));
    }
  }
  if (artifacts) await fs.writeFile(`${artifacts}/sarlacc-${mode}-track.json`, JSON.stringify(track));
  // Each one's end in the life he was put there in (a bot comes back).
  const first = track[0].world;
  const end = (who) => track.map((r) => r.world[who]).find((s) => s.life !== first[who].life || s.removed) ?? track.at(-1).world[who];
  const last = { me: end('me'), bot: end('bot'), faller: end('faller') };
  console.log(JSON.stringify({ mode, skin, seized, last, errors }));
  check(seized.me !== undefined && seized.bot !== undefined, 'both are seized', seized);
  // Held, the page draws each where the world holds him, tick for tick.
  for (const r of track) {
    if (r.world.me.held >= 0 && r.page.held) check(Math.hypot(...r.world.me.p.map((n, i) => n - r.page.me[i])) < 0.05, 'the page draws him where the world holds him', r);
    if (r.world.bot.held >= 0 && r.page.botHeld) check(Math.hypot(...r.world.bot.p.map((n, i) => n - r.page.bot[i])) < 0.05, 'and the bot', r);
  }
  check(track.some((r) => r.page.held && r.page.death), 'a seized player watches from his death camera (0550a3b2)', seized);
  for (const who of ['me', 'bot']) check(last[who].removed && last[who].dead && !last[who].killer, `${who} is swallowed, no one credited`, last[who]);
  check(last.faller.dead && !last.faller.killer, 'the pit (Sarlac01) kills whoever falls in, no one credited (his body lies there as any other)', last.faller);
  check(!errors.length, 'no page errors', errors);
  await battle.close();
} finally { await browser.close(); }
