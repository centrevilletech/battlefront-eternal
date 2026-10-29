import {EntityWorldCapture} from '../src/runtime/network-entity-capture.js';
import {EntityGameState} from '../src/runtime/network-entity-game.js';
import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync } from 'node:fs';
import * as THREE from 'three';
import { CollisionWorld } from '../src/collision.js';
import { Units } from '../src/registry.js';
import { UnitFactory } from '../src/units.js';

// One droideka class, built as a world worker or a dedicated server builds
// it (headless) and as the page that draws that world builds it. Synthetic
// model and clips; no retail data.
const look = { Color: [96, 160, 192, 255], SpecularColor: [216, 240, 255], Radius: 1.25, Height: 1.25, FadeIn: 0.15, FadeOut: 0.15 };
const def = {
  base: 'walkerdroid', model: 'deka', clips: 'deka_walk', MaxHealth: 300, HealthType: 'droid',
  weapons: [
    { class: 'repeater', channel: 0, aimers: ['hp_gun_1'] },
    { class: 'shield', channel: 1, base: 'shield', ShieldEffect: 'field', MaxShield: 4000, AddShield: -100, AddShieldOff: 300, ShieldOffset: [0, 1, 0], ShieldRadius: 1.25 },
  ],
};
const identity = new THREE.Matrix4().toArray();
const body = {
  def: {
    bones: [{ name: 'dummyroot', parent: -1, local: identity }, { name: 'center_box', parent: 0, local: identity }, { name: 'hp_gun_1', parent: 1, local: identity }],
    parts: [{ texture: 'deka', vertexCount: 3, vertexOffset: 0, indexCount: 3, indexOffset: 0 }],
    attach: 'hp_gun_1',
  },
  // Position, normal, uv and bone per vertex.
  buf: new Float32Array([0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 1, 0, 1, 0, 1, 0, 0, 1, 0, 0, 1, 2]).buffer,
  indexBuf: new Uint32Array([0, 1, 2]).buffer,
};
const frames = 13;
const pose = new Float32Array(frames * 7);
for (let i = 0; i < frames; i++) pose[i * 7 + 3] = 1;
const walk = {
  meta: { fps: 30, bones: ['dummyroot'], clips: Object.fromEntries(['idle', 'turnl', 'turnr', 'death01', 'idle_to_leftfoot', 'walk_leftfoot_rightfoot',
    'walk_rightfoot_leftfoot', 'leftfoot_to_idle', 'rightfoot_to_idle', 'fold', 'roll', 'unfold'].map((name) => [name, { frames, tracks: [0], offset: 0, flags: 1, travel: [0, 0, 0] }])) },
  buf: pose.buffer,
};
const ground = () => new CollisionWorld(new Float32Array([-100, 0, -100, 100, 0, -100, 100, 0, 100, -100, 0, 100]), new Uint32Array([0, 2, 1, 0, 3, 2]), new Uint32Array([0, 1]));

function battle(headless) {
  const units = new Units();
  const factory = new UnitFactory({ cis: { dir: '/sides/cis', units: { deka: def }, items: {}, texture: () => null, firePoints: {} } }, {
    graphics: { caster() {}, hiddenLayer: 1 }, scene: new THREE.Scene(), collision: ground(), animMeta: { fps: 30, clips: {}, sets: {} }, animBuf: new ArrayBuffer(0),
    ordnance: {}, lights: null, weaponModels: new Map(), flashModel: null, anyTexture: () => null, headless,
    effects: { shields: { field: look }, play() {} }, settings: { get: () => 'enhanced' }, items: null, live: null, units,
  });
  factory.loaded.set('body:cis/deka', body);
  factory.loaded.set('clips:cis/deka_walk', walk);
  return { units, factory, playerUnit: {}, battle: { characters: [] } };
}

function link(world,page) {
  const registry={classes:new Map([[1,'deka']]),weapons:new Map([[1,'repeater'],[2,'shield']])};
  const capture=new EntityWorldCapture(world,registry);
  const adapter=Object.create(EntityGameState.prototype);
  Object.assign(adapter,{game:page,registry,presentation:{bind(){}}});
  let tick=0;
  return ()=>{for(const r of capture.capture(++tick))if(r.kind==='droideka')adapter.droideka(r);};
}

test('a page keeps what it draws a body with when the world it shows is replicated onto it', () => {
  const world = battle(true), page = battle(false);
  const authority = world.factory.build('deka'), drawn = page.factory.build('deka');
  assert.equal(authority.id, drawn.id);
  // Whatever the drawing build adds that the headless one leaves empty is the
  // page's own: a packet must never take it away.
  drawn.jetFlame = { stop() {} };
  const own = Object.keys(drawn).filter((key) => drawn[key] && typeof drawn[key] === 'object' && !drawn[key].isVector3 && authority[key] == null);
  assert.ok(own.includes('bubble') && own.includes('jetFlame'));
  const kept = Object.fromEntries(own.map((key) => [key, drawn[key]]));
  const send = link(world, page);
  authority.spawn(3, 0, 4, 0);
  authority.soldier.toggleShield();
  send();
  for (const key of own) assert.equal(drawn[key], kept[key], `${key} survives the keyframe`);
  assert.equal(drawn.removed, false);
  assert.equal(drawn.soldier.shield.on, true);
  // The raised shield it was sent is drawn, around the body.
  drawn.presentation.reset();
  drawn.present(0.5, 1);
  assert.equal(drawn.bubble.mesh.visible, true);
  assert.ok(drawn.bubble.mesh.position.distanceTo(new THREE.Vector3(3, 1, 4)) < 1e-6);
  // Lowered in the world, it goes on the page. (The page's own frame let
  // go of the idle flame; it is the page's to keep or drop, not a packet's.)
  drawn.jetFlame = kept.jetFlame;
  authority.soldier.toggleShield();
  for (let i = 0; i < 3; i++) send();
  for (const key of own) assert.equal(drawn[key], kept[key], `${key} survives later packets`);
  drawn.present(1, 1);
  assert.equal(drawn.bubble.mesh.visible, false);
});

test('a unit off the field draws nothing, whatever its body was last told', () => {
  const page = battle(false), unit = page.factory.build('deka');
  unit.spawn(0, 0, 0, 0);
  unit.soldier.toggleShield();
  unit.present(0.5, 1);
  assert.equal(unit.actor.root.visible, true);
  assert.equal(unit.bubble.mesh.visible, true);
  // A packet that removes it but leaves its body shown (as a dedicated
  // server's own viewpoint did) would stand it unposed where it last was.
  unit.removed = true;
  unit.alive = false;
  unit.present(1 / 60, 1);
  assert.equal(unit.actor.root.visible, false);
  assert.equal(unit.bubble.mesh.visible, false);
  assert.equal(unit.bubble.fade, 0);
});

test('every life of a droideka starts with its shield full and down', () => {
  const world = battle(true), unit = world.factory.build('deka');
  unit.spawn(0, 0, 0, 0);
  assert.deepEqual([unit.soldier.shield.on, unit.soldier.shield.value], [false, 4000]);
  unit.soldier.toggleShield();
  unit.soldier.shield.value = 120;
  unit.remove();
  unit.spawn(5, 0, 5, 0);
  assert.deepEqual([unit.soldier.shield.on, unit.soldier.shield.value], [false, 4000]);
});

// A dedicated match (and a desktop host) retires the headless game's own
// viewpoint, which spawned at load from the URL's side. Its body must leave
// the world with it, or every client stands it at that spawn point.
const content = process.env.BFE_CONTENT_DIR || new URL('../public', import.meta.url).pathname;
test('a dedicated match leaves its own viewpoint body out of the world', { skip: !existsSync(`${content}/maps/bes2/map.json`) }, async () => {
  process.env.BFE_CONTENT_DIR ||= content;
  const { headlessGame, idleCommand } = await import('../tools/headless-world.mjs');
  const { createMultiplayerWorld } = await import('../src/runtime/multiplayer-world.js');
  const game = await headlessGame('?map=bes2&side=rep&ai=classic&seed=1');
  await game.ready;
  game.begin?.();
  for (let i = 0; i < 30; i++) game.step(idleCommand(game));
  const world = createMultiplayerWorld(game);
  for (let i = 0; i < 30; i++) world.step();
  assert.equal(game.playerUnit.removed, true);
  assert.equal(game.playerUnit.actor.root.visible, false);
});
