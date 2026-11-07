import {presentTurret, pageProps, local} from './helpers/page-presentation.js';
import {turretAt} from './fixtures/f3-turret.js';
import {f3World} from './fixtures/f3-world.js';
import {F4Live} from '../src/f4-live.js';
// Bots at turrets (docs/ai-vehicles.md "Turrets"): the drum's rest heading,
// its yaw limits, a bot's analogs turning the drum and gun onto a target
// wherever it stands, and damage to a turret reaching the AI as damage to
// its gunner. Synthetic fixtures only (no retail content).
import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { stepTurretPreview } from './helpers/network-preview.js';

const DEG = Math.PI / 180;
const wrap = (a) => Math.atan2(Math.sin(a), Math.cos(a));

// A turret at the origin whose instance faces +z (yaw `heading`): the drum
// 1.5 m up, bound at `drumBind` radians (Tatooine's barge turret binds its
// drum at 180 degrees), the gun on it, its fire point 1 m along the gun.

const barrel = (turret) => {
  const p = new THREE.Vector3(), d = new THREE.Vector3();
  turret.muzzle(p, d);
  return { p, d };
};

// The drum is posed along the aim from the building's frame, its bind turn
// not kept: a drum bound backward (the barge turret's) points its gun along
// the view all the same.
test("a turret's gun points along its view, whatever its drum's bind turn", () => {
  for (const drumBind of [0, Math.PI]) {
    const turret = turretAt({ heading: 30 * DEG, drumBind });
    presentTurret(turret);
    const { d } = barrel(turret);
    const view = turret.viewDirection(new THREE.Vector3());
    assert.ok(view.angleTo(d) < 1e-4, `bind ${drumBind}: the view is along the barrel (${view.angleTo(d)})`);
    turret.yaw = 1;
    presentTurret(turret);
    assert.ok(turret.viewDirection(view).angleTo(barrel(turret).d) < 1e-4, `bind ${drumBind}: and stays so as the drum turns`);
  }
});

test('the drum turns no further than YawLimits either way of its rest', () => {
  const turret = turretAt({ settings: { YawLimits: '-70 70' } });
  for (let i = 0; i < 120; i++) stepTurretPreview(turret, 1 / 60, 1, 0);
  assert.ok(Math.abs(turret.yaw - 70 * DEG) < 1e-9, `held at 70: ${turret.yaw / DEG}`);
  assert.equal(turret.yawVel, 0);
  for (let i = 0; i < 240; i++) stepTurretPreview(turret, 1 / 60, -1, 0);
  assert.ok(Math.abs(turret.yaw + 70 * DEG) < 1e-9);
  const free = turretAt({ settings: { YawLimits: '-180 180' } });
  for (let i = 0; i < 120; i++) stepTurretPreview(free, 1 / 60, 1, 0);
  assert.ok(Math.abs(wrap(free.yaw - 2 * 2.85)) < 0.3, 'all the way round');
});

test("damage to a turret is reported with the round's heading and factors", async () => {
  const turret = turretAt();
  const seen = [];
  turret.onDamaged = (object, amount, attacker, hit) => seen.push({ object, amount, attacker, hit });
  const w=await f3World({soldier:{class:'soldier',base:'soldier',MaxHealth:100,weapons:[]}},
    {prepareLive:()=>({buildings:[turret],turrets:[turret],animated:[],traps:[]})});
  new F4Live(w.phase,{sound(){}});
  const attacker=w.spawn('soldier',20,20);attacker.team=attacker.combat.team=2;
  const direction = new THREE.Vector3(0, 0, -1);
  turret.damage(5, attacker, { direction, amount: 50, typed: 0.1, generic: 1 });
  assert.equal(seen.length, 1);
  assert.equal(seen[0].amount, 5);
  assert.equal(seen[0].attacker, attacker);
  assert.deepEqual(seen[0].hit.direction.toArray(), direction.toArray());
  assert.equal(turret.health, 295);
  turret.damage(0, attacker);
  assert.equal(seen.length, 1, 'nothing for no damage');
  turret.damage(1000, attacker);
  turret.damage(5, attacker);
  assert.equal(seen.length, 2, 'nothing once destroyed');
});
