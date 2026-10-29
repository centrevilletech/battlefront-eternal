import test from "node:test";
import assert from "node:assert/strict";
import * as THREE from "three";
import { LightPool, Ordnance } from "../src/ordnance.js";

test("light requests retain each emitter position when callers reuse scratch vectors", () => {
  const pool = new LightPool(new THREE.Scene(), 3), at = new THREE.Vector3();
  for (let i = 0; i < 3; i++) pool.add(at.set(i * 10, 2, 0), [1, .2, .1], 3);
  at.set(999, 999, 999); pool.update(0);
  assert.deepEqual(pool.lights.map(l => l.position.x).sort((a,b) => a-b), [0, 10, 20]);
  pool.add(at.set(42, 1, 0), [0, 1, 0], 5); pool.update(0);
  assert.equal(pool.lights[0].position.x, 42);
  assert.equal(pool.lights[1].intensity, 0);
});

test("visible bolt tail starts at the view muzzle and joins the collision path", () => {
  const o = new Ordnance({ scene: new THREE.Scene(), collision: { cast: () => null },
    effects: {}, lights: { add() {} }, texture: () => null });
  const origin = new THREE.Vector3(0, 1, 0), muzzle = new THREE.Vector3(.7, .8, .3);
  const def = { Velocity: 100, LifeSpan: 2, LaserLength: 3 };
  o.bolt(def, origin, new THREE.Vector3(0, 0, 1), "player", muzzle);
  const b = o.bolts[0]; o.stepBolt(b, .01);
  const length = o.boltSpan(b, b.flown);
  const tail = o.drawnAt.clone().addScaledVector(new THREE.Vector3(...o.boltAxis), -length);
  assert.ok(tail.distanceTo(muzzle) < 1e-8);
  o.stepBolt(b, .12);
  const joinedLength = o.boltSpan(b, 2);
  assert.equal(joinedLength, 2);
  assert.deepEqual(o.boltAxis, [0, 0, 1]);
  assert.deepEqual(b.position.toArray(), [0, 1, 13]);
});

test("bolt draw resources are reused without per-frame texture lookups", () => {
  let lookups = 0;
  const texture = new THREE.DataTexture(new Uint8Array([255, 20, 5, 255]), 1, 1);
  const o = new Ordnance({ scene: new THREE.Scene(), collision: { cast: () => null },
    effects: {}, lights: { add() {} }, texture: () => { lookups++; return texture; } });
  const def = { Velocity: 50, LifeSpan: 10, LaserLength: 2, LaserWidth: .1, GlowLength: 3, LaserTexture: "red" };
  for (let n = 0; n < 10; n++) o.bolt(def, new THREE.Vector3(n, 0, 0), new THREE.Vector3(0, 0, 1), "player");
  for (let i = 0; i < 120; i++) o.update(1 / 60);
  assert.equal(lookups, 2);
  assert.equal(o.renderer.batches.size, 2);
  assert.equal([...o.renderer.batches.values()].reduce((n,b) => n+b.count, 0), 30);
});

test('a delayed first draw starts at the posed muzzle without moving the collision ray', async () => {
  const { boltLook } = await import('../src/ordnance.js');
  const enhanced = boltLook.enhanced;
  try {
    for (const style of [false, true]) {
      boltLook.enhanced = style;
      const o = new Ordnance({ scene: new THREE.Scene(), collision: { cast: () => null },
        effects: {}, lights: { add() {} }, texture: () => new THREE.Texture() });
      const tails = [];
      o.renderer.batch = () => ({ push(x, y, z, r, g, b, a, width, angle, mode, length, axis) {
        tails.push(new THREE.Vector3(x, y, z).addScaledVector(new THREE.Vector3(...axis), -length));
      } });
      o.renderer.custom = () => ({ push(head, tail) { tails.push(tail.clone()); } });
      const muzzle = new THREE.Vector3(.6, 1.4, .8);
      const bolt = o.bolt({ Velocity: 150, LifeSpan: 2, LaserLength: 1, GlowLength: 3, LaserTexture: 'laser' },
        new THREE.Vector3(0, 1.7, 0), new THREE.Vector3(0, 0, 1), 'player');
      bolt.visualMuzzle = () => muzzle;
      for (let tick = 0; tick < 4; tick++) o.stepBolt(bolt, 1 / 60, false);
      const physical = [bolt.position.toArray(), bolt.direction.toArray(), bolt.flown, bolt.life];
      o.present(.5);
      assert.ok(tails.length > 0);
      for (const tail of tails) assert.ok(tail.distanceTo(muzzle) < 1e-8, `style ${style}: first tail missed muzzle`);
      assert.deepEqual([bolt.position.toArray(), bolt.direction.toArray(), bolt.flown, bolt.life], physical);
      tails.length = 0;
      o.stepBolt(bolt, 1 / 60, false); o.present(1);
      for (const tail of tails) assert.ok(tail.distanceTo(muzzle) > 1, 'later streaks leave the muzzle');
    }
  } finally { boltLook.enhanced = enhanced; }
});

test('mounted bolt first draw follows the posed barrel then detaches', () => {
  const o = new Ordnance({ scene: new THREE.Scene(), collision: { cast: () => null }, effects: {}, lights: { add() {} }, texture: () => null });
  const node = new THREE.Object3D(); node.position.set(1, 2, 3);
  const bolt = o.bolt({ Velocity: 100, LifeSpan: 2, LaserLength: 3 }, new THREE.Vector3(), new THREE.Vector3(0, 0, 1), null);
  Object.assign(bolt, { mount: { alive: true, firePoints: [node] }, mountSeat: -1, mountBarrel: 0 });
  o.stepBolt(bolt, .01, false);
  o.present();
  assert.ok(bolt.offset.distanceTo(node.position) < 1e-8);
  const offset = bolt.offset.clone(); node.position.x = 9;
  o.present();
  assert.ok(bolt.offset.equals(offset), 'the emitted bolt must not keep following its gun');
});

test('launched missile mesh follows its displayed muzzle without moving collision state', () => {
  const o = new Ordnance({ scene: new THREE.Scene(), collision: { cast: () => null }, effects: {}, lights: { add() {} }, texture: () => null, model: () => () => new THREE.Group() });
  const origin = new THREE.Vector3(0, 1, 0), muzzle = new THREE.Vector3(.5, .8, 1);
  const round = o.fire({ base: 'missile', GeometryName: 'rocket', Velocity: 10, LifeSpan: 3 }, origin, new THREE.Vector3(0, 0, 1), null);
  round.visualMuzzle = () => muzzle;
  o.present();
  assert.ok(round.mesh.position.distanceTo(muzzle) < 1e-8);
  assert.ok(round.position.equals(origin));
  round.previous.copy(round.position); round.position.z = 10;
  o.present();
  assert.ok(round.mesh.position.equals(round.position), 'after convergence the mesh follows the collision path');
});

test('a beam leaves its ImpactEffect on whatever it touches; an older extract keeps the surface effect', () => {
  const played = [];
  const make = (cast, raycast = () => null) => {
    const o = new Ordnance({ scene: new THREE.Scene(), collision: { cast }, effects: { play: name => played.push(name) },
      lights: { add() {} }, texture: () => null });
    o.castUnits = raycast;
    return o;
  };
  const ground = (from, dir, far, hit) => Object.assign(hit, { distance: 12, terrain: true, body: null });
  // cis_weap_walk_spider_laser_ord: spiderdroid_ord_exp (ImpactEffectTerrain is the port's dirtspray default).
  const laser = { base: 'beam', LifeSpan: .4, Damage: 100, ImpactEffect: 'spiderdroid_ord_exp', ImpactEffectTerrain: 'dirtspray' };
  let o = make(ground);
  o.beam(laser, new THREE.Vector3(), new THREE.Vector3(0, 0, 1), 'player');
  o.stepBeam(o.beams[0], 1 / 60, false);
  assert.deepEqual(played, ['spiderdroid_ord_exp']);
  played.length = 0;
  o = make(() => null, () => ({ unit: { id: 3 }, distance: 8, part: 'body' }));
  o.onHitUnit = () => {};
  o.beam(laser, new THREE.Vector3(), new THREE.Vector3(0, 0, 1), 'player');
  o.stepBeam(o.beams[0], 1 / 60, false);
  assert.deepEqual(played, ['spiderdroid_ord_exp'], 'a soldier or hull struck');
  played.length = 0;
  o = make(ground);
  o.beam({ ...laser, ImpactEffect: undefined }, new THREE.Vector3(), new THREE.Vector3(0, 0, 1), 'player');
  o.stepBeam(o.beams[0], 1 / 60, false);
  assert.deepEqual(played, ['dirtspray']);
});

test('a replicated beam without its draw scratch still draws', () => {
  const texture = new THREE.DataTexture(new Uint8Array([255, 20, 5, 255]), 1, 1);
  const o = new Ordnance({ scene: new THREE.Scene(), collision: { cast: () => null }, effects: {}, lights: { add() {} }, texture: () => texture });
  // As a worker's or server's beam reaches the page: no axis, no colour.
  const beam = { def: { LaserGlowColor: [255, 0, 0, 255], LaserWidth: .4, LightRadius: 3 }, life: .2,
    position: new THREE.Vector3(0, 1, 0), end: new THREE.Vector3(0, 1, 30), direction: new THREE.Vector3(0, 0, 1) };
  assert.doesNotThrow(() => o.drawBeam(beam));
  assert.deepEqual(beam.axis, [0, 0, 1]);
});

test('a bolt class that names no GlowLength glows three times its core, white without a colour', () => {
  const texture = new THREE.DataTexture(new Uint8Array([255, 20, 5, 255]), 1, 1);
  const o = new Ordnance({ scene: new THREE.Scene(), collision: { cast: () => null }, effects: {}, lights: { add() {} }, texture: () => texture });
  // A turret gun's bolt: LaserLength and width, no GlowLength, no colour.
  const def = { Velocity: 50, LifeSpan: 10, LaserLength: 2, LaserWidth: .1, LaserTexture: "red" };
  o.bolt(def, new THREE.Vector3(), new THREE.Vector3(0, 0, 1), "player");
  for (let i = 0; i < 30; i++) o.update(1 / 60);
  const bolt = o.bolts[0];
  assert.ok(Math.abs(bolt.drawn - 6) < 1e-9, 'drawn out to the glow, 3 x LaserLength');
  const glow = [...o.renderer.batches.entries()].find(([key]) => !String(key).includes('bolt'))?.[1];
  assert.ok(glow?.count > 0, 'the glow is drawn');
});

test('a sniper bolt is drawn on where it stopped for its FadeOutTime, fading and thinning to a tenth', () => {
  const pushes = [], lights = [];
  const o = new Ordnance({ scene: new THREE.Scene(), collision: { cast: () => null }, effects: {},
    lights: { add: (at, rgb, radius) => lights.push({ at: at.toArray(), rgb: [...rgb], radius }) }, texture: () => new THREE.Texture() });
  o.renderer.batch = (texture, blending, profile) => ({ push: (x, y, z, r, g, b, a, size, rotation, shape, length) => pushes.push({ profile, head: [x, y, z], rgb: [r, g, b], a, size, length }) });
  // rep_weap_inf_sniperrifle_ord, as the page has it from the world: fired
  // on tick 10, its round stopped 50 m out (1.5 ticks at 2000 m/s).
  const def = { Velocity: 2000, LifeSpan: .15, LaserTexture: 'bluelaser_d', LaserLength: 20, LaserWidth: .15, GlowLength: 24, BlurLength: 32,
    FadeOutTime: 1, LaserGlowColor: [92, 136, 250, 100], LightColor: [92, 136, 250, 100], LightRadius: 4 };
  const round = def => ({ def, position: new THREE.Vector3(), direction: new THREE.Vector3(0, 0, 1), offset: new THREE.Vector3(),
    speed: 2000, life: .025, merge: 8, flown: 0, drawn: 0, networkFlightTick: 10, presented: true });
  const draw = (source, tick) => {
    pushes.length = lights.length = 0;
    o.drawBolt(source, 1, tick);
    return { core: pushes.find((p) => p.profile === 'bolt'), glow: pushes.find((p) => p.profile !== 'bolt'), light: lights[0] };
  };
  const close = (a, b, message) => assert.ok(Math.abs(a - b) < 1e-9, `${message}: ${a} against ${b}`);
  const sniper = round(def);
  let { core, glow, light } = draw(sniper, 11);
  close(core.head[2], 2000 / 60, 'in flight');
  assert.equal(core.a, 1); close(core.size, .075, 'full width in flight'); close(core.length, 20, 'the core');
  assert.deepEqual(light.at, core.head, 'its light rides at the head');
  // Its flight is over: the core and glow end where it stopped, as long as
  // they are, full strength at first.
  ({ core, glow, light } = draw(sniper, 11.5));
  assert.deepEqual(core.head, [0, 0, 50]); close(core.length, 20, 'the core'); close(glow.length, 24, 'the glow');
  assert.equal(core.a, 1); close(core.size, .075, 'full width');
  // Half a second on: half its strength, its width down to 0.55, the glow's
  // colour fading as its alpha does; its light still lit, at the middle of
  // its last step.
  ({ core, glow, light } = draw(sniper, 41.5));
  assert.deepEqual(core.head, [0, 0, 50]);
  close(core.a, .5, 'alpha'); close(core.size, .075 * .55, 'width'); close(glow.a, .5, 'glow alpha');
  close(glow.rgb[2], (250 / 255) * (100 / 255) * 1.35 * .5, 'glow colour');
  close(light.at[2], 50 - 2000 / 120, 'light'); assert.equal(light.radius, 4);
  // Gone after a second, and a class without a FadeOutTime is gone when it stops.
  assert.equal(draw(sniper, 72).core, undefined);
  assert.equal(draw(round({ ...def, FadeOutTime: undefined }), 11.5).core, undefined);
  // A shorter shot is drawn all the way back to where it was fired.
  const close10 = { ...round(def), life: .005 };
  ({ core } = draw(close10, 20));
  assert.deepEqual(core.head, [0, 0, 10]); close(core.length, 10, 'as far as it flew');
});

test("the light pool hands its lights to the heaviest requests, in a stable order, without new arrays each frame", () => {
  const pool = new LightPool(new THREE.Scene(), 3), at = new THREE.Vector3();
  const timed = pool.timed;
  // Weights (rgb sum x scale x radius): 3, 9, 9, 1, 12, 3.
  for (const [x, radius] of [[1, 1], [2, 3], [3, 3], [4, 1 / 3], [5, 4], [6, 1]]) pool.add(at.set(x, 0, 0), [1, 1, 1], radius);
  pool.flash(at.set(7, 0, 0), [0.1, 0.1, 0.1], 1, 1);
  pool.update(0.5);
  assert.deepEqual(pool.lights.map((l) => l.position.x), [5, 2, 3], "heaviest first, ties in the order asked");
  assert.equal(pool.timed, timed, "the same list, kept in place");
  assert.equal(timed.length, 1);
  pool.update(0.6);
  assert.equal(timed.length, 0, "its flash is over");
  assert.deepEqual(pool.lights.map((l) => l.intensity), [0, 0, 0], "nothing asked: all dark, none removed");
  assert.equal(pool.requestCount, 0);
});
