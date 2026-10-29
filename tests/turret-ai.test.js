// Bots at turrets (docs/ai-vehicles.md "Turrets"): the drum's rest heading,
// its yaw limits, a bot's analogs turning the drum and gun onto a target
// wherever it stands, and damage to a turret reaching the AI as damage to
// its gunner. Synthetic fixtures only (no retail content).
import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { CollisionWorld } from '../src/collision.js';
import { Turret } from '../src/props.js';
import { TurretSeats } from '../src/seats.js';
import { stationaryInput } from '../src/ai/classic/vehicles/steering.js';

const DEG = Math.PI / 180;
const wrap = (a) => Math.atan2(Math.sin(a), Math.cos(a));

function floor() {
  const g = new THREE.BoxGeometry(400, 2, 400).translate(0, -1, 0);
  return new CollisionWorld(g.attributes.position.array, g.index.array, new Uint32Array(g.index.count / 3));
}

// A turret at the origin whose instance faces +z (yaw `heading`): the drum
// 1.5 m up, bound at `drumBind` radians (Tatooine's barge turret binds its
// drum at 180 degrees), the gun on it, its fire point 1 m along the gun.
function turretAt({ heading = 0, drumBind = 0, settings = {} } = {}) {
  const rig = { root: new THREE.Group(), byName: {} };
  for (const name of ['drum', 'gun', 'seat', 'hp_fire']) { const o = new THREE.Object3D(); o.name = name; rig.byName[name] = o; }
  rig.root.add(rig.byName.drum); rig.byName.drum.add(rig.byName.gun); rig.byName.gun.add(rig.byName.hp_fire); rig.byName.drum.add(rig.byName.seat);
  rig.byName.drum.position.y = 1.5; rig.byName.drum.quaternion.setFromAxisAngle(THREE.Object3D.DEFAULT_UP, drumBind);
  rig.byName.hp_fire.position.z = 1; rig.byName.seat.position.z = -0.6;
  rig.model = { box: [[-1, 0, -1], [1, 2, 1]] };
  const matrix = new THREE.Matrix4().makeRotationY(heading);
  rig.root.applyMatrix4(matrix);
  const desc = { base: 'armedbuilding', class: 'test_turret', matrix: matrix.toArray(),
    settings: { TurretNodeName: 'drum', AimerNodeName: 'gun', PilotPosition: 'seat', FirePointName: 'hp_fire', MaxTurnSpeed: 2.85, MaxHealth: 300, AimerYawLimits: '-5 5', ...settings },
    weapon: { class: 'test_turret_gun', base: 'cannon', ShotDelay: 0.4, RoundsPerClip: 0, ReloadTime: 0, ordnance: { base: 'bolt', Velocity: 80, LifeSpan: 2, Damage: 20 } } };
  return new Turret(desc, { built: rig }, { built: null }, null);
}

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
    turret.present();
    const { d } = barrel(turret);
    const view = turret.viewDirection(new THREE.Vector3());
    assert.ok(view.angleTo(d) < 1e-4, `bind ${drumBind}: the view is along the barrel (${view.angleTo(d)})`);
    turret.yaw = 1;
    turret.present();
    assert.ok(turret.viewDirection(view).angleTo(barrel(turret).d) < 1e-4, `bind ${drumBind}: and stays so as the drum turns`);
  }
});

test('the drum turns no further than YawLimits either way of its rest', () => {
  const turret = turretAt({ settings: { YawLimits: '-70 70' } });
  for (let i = 0; i < 120; i++) turret.steer(1 / 60, 1, 0);
  assert.ok(Math.abs(turret.yaw - 70 * DEG) < 1e-9, `held at 70: ${turret.yaw / DEG}`);
  assert.equal(turret.yawVel, 0);
  for (let i = 0; i < 240; i++) turret.steer(1 / 60, -1, 0);
  assert.ok(Math.abs(turret.yaw + 70 * DEG) < 1e-9);
  const free = turretAt({ settings: { YawLimits: '-180 180' } });
  for (let i = 0; i < 120; i++) free.steer(1 / 60, 1, 0);
  assert.ok(Math.abs(wrap(free.yaw - 2 * 2.85)) < 0.3, 'all the way round');
});

// A bot at the gun: its stationary analogs (the UnitStationaryController's
// input conversion) every frame toward a point, TurretSeats driving the
// drum, the gun following the aim (here straight at the point, as the
// Aimer settles).
function botAt(turret) {
  const collision = floor();
  const crews = new TurretSeats({ live: { turrets: [turret] }, collision, ordnance: { fire() { return null; } } });
  const soldier = { position: new THREE.Vector3(0, 0, 5), previous: new THREE.Vector3(), settled: new THREE.Vector3(), velocity: new THREE.Vector3(), aimYaw: 0, aimPitch: 0 };
  const unit = { id: 9, generation: 1, team: 1, alive: true, dead: false, human: false, walker: false, seat: null, controller: null, classDef: {},
    intent: { forward: 0, right: 0, turn: 0, pitch: 0, aim: new THREE.Vector3(), buttons: [0, 0] }, soldier,
    actor: { root: new THREE.Group(), byName: {} }, animator: { clips: new Map(), gunner() {}, reset() {} }, presentation: { reset() {} },
    combat: { mounted: false, dead: false, team: 1, damage() {} } };
  assert.ok(crews.man(unit, turret));
  return { crews, unit, seat: turret.lease.controllable };
}

for (const [label, heading, drumBind] of [['an ordinary turret', 0, 0], ['a drum bound backward', 0.7, Math.PI]]) {
  // (Not exactly behind: the original game's ordinary aim conversion turns by
  // the sideways cross product, which is nothing there; any movement of the
  // target or the drum breaks that balance.)
  test(`a bot turns ${label} onto a target behind, beside and ahead of it`, () => {
    for (const bearing of [175, -170, 90, -120, 10]) {
      const turret = turretAt({ heading, drumBind });
      const { crews, unit, seat } = botAt(turret);
      const yaw = turret.facing + bearing * DEG;
      const target = new THREE.Vector3(Math.sin(yaw) * 30, 2, Math.cos(yaw) * 30);
      const out = { forward: 0, right: 0, turn: 0, pitch: 0 };
      const origin = new THREE.Vector3();
      let within = null;
      for (let n = 1; n <= 150; n++) {
        stationaryInput(seat, null, target, 1 / 60, {}, out);
        unit.intent.turn = out.turn;
        unit.intent.pitch = out.pitch;
        seat.origin(origin);
        unit.intent.aim.subVectors(target, origin).normalize();
        crews.update(1 / 60);
        const { p, d } = barrel(turret);
        if (within === null && d.angleTo(target.clone().sub(p)) < 2 * DEG) within = n / 60;
      }
      const { p, d } = barrel(turret);
      const err = d.angleTo(target.clone().sub(p)) / DEG;
      assert.ok(err < 1, `${label}, bearing ${bearing}: barrel ${err.toFixed(2)} degrees off`);
      // 180 degrees at 2.85 rad/s is 1.1 s, plus the turn filter's ease.
      assert.ok(within !== null && within < 1.6, `${label}, bearing ${bearing}: on target after ${within} s`);
    }
  });
}

test("damage to a turret is reported with the round's heading and factors", () => {
  const turret = turretAt();
  const seen = [];
  turret.onDamaged = (object, amount, attacker, hit) => seen.push({ object, amount, attacker, hit });
  const attacker = { team: 2 };
  const direction = new THREE.Vector3(0, 0, -1);
  turret.damage(5, attacker, { direction, amount: 50, typed: 0.1, generic: 1 });
  assert.equal(seen.length, 1);
  assert.equal(seen[0].amount, 5);
  assert.equal(seen[0].attacker, attacker);
  assert.equal(seen[0].hit.direction, direction);
  assert.equal(turret.health, 295);
  turret.damage(0, attacker);
  assert.equal(seen.length, 1, 'nothing for no damage');
  turret.damage(1000, attacker);
  turret.damage(5, attacker);
  assert.equal(seen.length, 2, 'nothing once destroyed');
});
