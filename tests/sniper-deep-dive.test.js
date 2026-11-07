import {rustOrdnance} from './helpers/rust-simulation.js';
import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { Vector3, Scene } from 'three';
import { Units } from '../src/registry.js';
import { lighting } from '../src/materials.js';

const sniperDef = {
  class: 'test_sniper', base: 'cannon', channel: 0, ammo: 7, RoundsPerClip: 6, ReloadTime: 2, ShotDelay: 1, TriggerSingle: 1,
  YawSpread: 0, PitchSpread: 0, ordnance: { class: 'test_sniper_ord', base: 'bolt', Velocity: 2000, LifeSpan: 0.15, FadeOutTime: 1, Damage: 160 },
};
test('droidekas are struck through p_ord and p_rollsphere, with no head', async () => {
  const {f3World}=await import('./fixtures/f3-world.js');
  const world=await f3World({droideka:{class:'droideka',base:'walkerdroid',MaxHealth:100,weapons:[]}});
  const unit=world.spawn('droideka',0,0),s=unit.soldier;
  const shapes = unit.hitShapes();
  assert.equal(shapes.length, 2);
  assert.ok(shapes.every((shape) => shape.part === 'body'));
  const ord = shapes.find((shape) => shape.type === 'cylinder');
  assert.deepEqual(ord.center.toArray().map((n) => +n.toFixed(3)), [0, 1, 0.506]);
  assert.equal(ord.radius, 0.8); assert.equal(ord.half, 1);
  const folded=await f3World({droideka:{class:'droideka',base:'walkerdroid',MaxHealth:100,weapons:[]}},
    {passiveClasses:['droideka'],prepareUnit(u){u.alive=true;u.removed=false;u.soldier.mode=12;}});
  assert.equal(folded.passive[0].hitShapes().length,1,'rolled, the ball alone');
});

test('the crosshair ray converges on a soldier under it, not the ground behind him', async () => {
  const { PlayerCombat } = await import('../src/combat.js');
  const { CombatWorld } = await import('../src/combat-world.js');
  const self = { collision: { cast: () => null }, ordnance: null, crosshairReach: PlayerCombat.prototype.crosshairReach };
  self.world = new CombatWorld(self);
  const from = new Vector3(0, 2, 0), forward = new Vector3(0, 0, 1);
  assert.equal(self.crosshairReach(from, forward), 60, 'nothing in the way: 60 m');
  const units = new Units();
  const target = units.add({ alive: true, generation: 1, combat: {}, position: new Vector3(0, 0, 40), get sphere() { return { center: new Vector3(0, 2, 40), radius: 1 }; },
    hitShapes() { return [{ type: 'sphere', center: new Vector3(0, 2, 40), radius: 0.17, part: 'head' }]; } });
  units.add({ alive: true, generation: 1, combat: self, position: new Vector3(0, 0, 1), get sphere() { return { center: new Vector3(0, 2, 1), radius: 1 }; },
    hitShapes() { return [{ type: 'sphere', center: new Vector3(0, 2, 1), radius: 0.5, part: 'body' }]; } });
  self.ordnance = { units };
  assert.ok(Math.abs(self.crosshairReach(from, forward) - 39.83) < 1e-6, 'onto his head, the shooter himself left out');
  target.alive = false;
  assert.equal(self.crosshairReach(from, forward), 60, 'a body lying dead is not aimed at');
});

test('the Enhanced sniper resources compile during battle warm-up', () => {
  const scene = new Scene();
  const ordnance = rustOrdnance({ scene, collision: { cast: () => null }, effects: {}, lights: { add() {}, flash() {} }, texture: () => null });
  ordnance.prepare();
  const names=[];scene.traverse(o=>names.push(o.name));
  assert.ok(names.includes('sniper-trace')&&names.includes('sniper-flares'));
  const trace=ordnance.sniperTrace();lighting.time.value=50;trace.begin();trace.end();
  assert.equal(trace.streaks.geometry.instanceCount,1);
  assert.equal(trace.flares.geometry.instanceCount,1);
  assert.equal(trace.streaks.capacity,512);
  assert.equal(trace.flares.capacity,128);
});
