import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { ShieldBubble } from '../src/droideka-shield.js';
import { Droideka } from '../src/droideka.js';
import { PlayerCombat } from '../src/combat.js';

const look = { Color: [96,160,192,255], SpecularColor: [216,240,255], Radius: 1.25, Height: 1.25, FadeIn: 0.15, FadeOut: 0.15 };
function fixture(mode = 'classic') {
  const d = Object.assign(Object.create(Droideka.prototype), { position: new THREE.Vector3(0,0,0), aimYaw: 0, mode: 0, dead: false,
    shield: { on: false, value: 4000, max: 4000, def: { ShieldOffset: [0,1,0.125], ShieldRadius: 1.25 } } });
  const settings = { mode, get() { return this.mode; } };
  const bubble = new ShieldBubble(new THREE.Scene(), look, () => null, settings);
  d.onShieldHit = (amount, source, kind) => bubble.hit(source, amount, d, kind);
  return { d, settings, bubble };
}

test('Classic uses native activation growth, overshoot, dimensions and collapse timing', () => {
  const { d, bubble: b } = fixture(); d.shield.on = true;
  b.update(0.15 * 0.47, d);
  assert.ok(Math.abs(b.mesh.scale.x / 1.25 - 0.53) < 1e-6);
  b.update(0.15 * 0.47, d);
  assert.ok(Math.abs(b.mesh.scale.x / 1.25 - 1.06) < 1e-6);
  b.update(0.15 * 0.06, d);
  assert.ok(Math.abs(b.mesh.scale.x - 1.25) < 1e-6);
  assert.deepEqual(b.mesh.position.toArray(), [0,1,0.125]);
  d.shield.on = false; b.update(0.075, d);
  assert.ok(Math.abs(b.mesh.scale.x - 0.625) < 1e-6);
  b.update(0.075, d); assert.equal(b.mesh.visible, false); b.dispose();
});

test('Enhanced pop remains continuous through interrupted activation and resets after collapse', () => {
  const { d, bubble: b } = fixture('enhanced'); d.shield.on = true; b.update(0.07, d);
  let scale = b.mesh.scale.x; d.shield.on = false; b.update(0, d);
  assert.ok(Math.abs(b.mesh.scale.x - scale) < 1e-8);
  b.update(0.03, d); scale = b.mesh.scale.x; d.shield.on = true; b.update(0, d);
  assert.ok(Math.abs(b.mesh.scale.x - scale) < 1e-8);
  b.update(1, d); assert.equal(b.mesh.scale.x, 1.25);
  d.shield.on = false; b.update(1, d); d.shield.on = true; b.update(0, d);
  assert.ok(b.mesh.scale.x < 0.01); b.dispose();
});

test('real absorption forwards damage source and keeps a bounded localized impact pool', () => {
  const { d, bubble: b } = fixture('enhanced'); d.shield.on = true; b.update(0.3, d);
  const from = new THREE.Vector3(5,1,0.125);
  const combat = { dead: false, soldier: d, health: 100 };
  PlayerCombat.prototype.damage.call(combat, 40, 100, from);
  assert.equal(combat.health, 100); assert.equal(d.shield.value, 3900);
  assert.ok(b.uniforms.impacts.value[0].x > 0.999);
  assert.ok(b.uniforms.powers.value[0] > 0);
  const pool = b.uniforms.impacts.value;
  for (let i = 0; i < 30; i++) b.hit(from, 10, d);
  assert.equal(pool, b.uniforms.impacts.value); assert.equal(pool.length, 6);
  b.dispose();
});

test('drain, regeneration and unabsorbed damage cannot fabricate hit animations', () => {
  const { d, bubble: b } = fixture(); d.shield.on = true;
  for (let i = 0; i < 20; i++) { d.shield.value -= 10; b.update(1 / 60, d); }
  d.absorb(10, 0); d.shield.on = false; d.absorb(10, 100);
  assert.equal(b.hitIndex, 0); assert.equal(b.flash, 0);
  d.shield.on = true; d.absorb(10, 100);
  assert.ok(b.flash > 0, 'unknown damage source gives an undirected flash');
  assert.equal(b.hitIndex, 0, 'no fabricated localized impact'); b.dispose();
});

test('presentation follows rendered feet, switches modes live and disappears rolled/dead', () => {
  const { d, settings, bubble: b } = fixture(); d.shield.on = true; d.aimYaw = Math.PI / 2;
  b.update(0.3, d, { position: new THREE.Vector3(3,2,4) });
  assert.ok(b.mesh.position.distanceTo(new THREE.Vector3(3.125,3,4)) < 1e-8);
  assert.equal(b.uniforms.enhanced.value, 0);
  settings.mode = 'enhanced'; b.update(0, d); assert.equal(b.uniforms.enhanced.value, 1);
  d.mode = 12; b.update(1, d); assert.equal(b.mesh.visible, false);
  d.mode = 0; d.dead = true; b.update(1, d); assert.equal(b.mesh.visible, false); b.dispose();
});

function blastFixture(mode = 'enhanced') {
  const fixtureResult = fixture(mode), { d, bubble } = fixtureResult;
  Object.assign(d, { velocity: new THREE.Vector3(1, 0, 2), grounded: true, air: null });
  d.shield.on = true; bubble.update(2, d);
  const combat = Object.assign(Object.create(PlayerCombat.prototype), {
    soldier: d, def: { base: 'walkerdroid' }, dead: false, health: 100,
    worldPoint: new THREE.Vector3(), shake: {}, collision: { cast: () => null },
    scale: 'DroidScale', onDamaged() {},
  });
  const explosion = { Damage: 40, DamageRadiusInner: 5, DamageRadiusOuter: 10,
    Push: 30, PushRadiusInner: 5, PushRadiusOuter: 10 };
  return { ...fixtureResult, combat, explosion, source: new THREE.Vector3(2, 0, 0) };
}

test('blast absorption produces an explosion shield reaction without launching the droideka', () => {
  const { d, bubble, combat, explosion, source } = blastFixture();
  combat.explode(explosion, source);
  assert.equal(combat.health, 100); assert.equal(d.shield.value, 3960);
  assert.deepEqual(d.velocity.toArray(), [1, 0, 2]);
  assert.equal(d.grounded, true); assert.equal(d.air, null);
  assert.equal(bubble.uniforms.blasts.value[0], 1);
  assert.ok(bubble.uniforms.impacts.value[0].x > 0);
  bubble.dispose();
});

test('unshielded and rolling droidekas take blast damage without being thrown', () => {
  for (const mode of [0, 12]) {
    const { d, bubble, combat, explosion, source } = blastFixture();
    d.shield.on = false; d.mode = mode;
    combat.explode(explosion, source);
    assert.equal(combat.health, 60);
    assert.deepEqual(d.velocity.toArray(), [1, 0, 2]);
    assert.equal(d.grounded, true); assert.equal(d.air, null);
    assert.equal(bubble.hitIndex, 0); bubble.dispose();
  }
});

test('blocked and friendly-fire-disabled explosions cannot trigger shield absorption', () => {
  for (const blocked of [false, true]) {
    const { d, bubble, combat, explosion, source } = blastFixture();
    if (blocked) combat.collision.cast = () => ({ distance: 0.1 });
    combat.explode(explosion, source, null, blocked ? 1 : 0);
    assert.equal(d.shield.value, 4000); assert.equal(bubble.hitIndex, 0);
    assert.deepEqual(d.velocity.toArray(), [1, 0, 2]); bubble.dispose();
  }
});

test('infantry still receive explosion knockback', () => {
  const { d, bubble, combat, explosion, source } = blastFixture();
  combat.def.base = 'soldier';
  combat.explode(explosion, source);
  assert.ok(d.velocity.y > 0); assert.equal(d.air, 'thrown');
  assert.equal(d.grounded, false); bubble.dispose();
});

test('a shield-breaking blast animates the absorbed portion and passes remaining damage to health', () => {
  const { d, bubble, combat, explosion, source } = blastFixture();
  d.shield.value = 10;
  combat.explode(explosion, source);
  assert.equal(d.shield.value, 0); assert.equal(d.shield.on, false);
  assert.equal(combat.health, 70); assert.equal(bubble.uniforms.blasts.value[0], 1);
  assert.deepEqual(d.velocity.toArray(), [1, 0, 2]);
  bubble.update(0.05, d); assert.equal(bubble.mesh.visible, true);
  bubble.update(1, d); assert.equal(bubble.mesh.visible, false);
  bubble.dispose();
});
