import {weaponCheckpoint} from './helpers/weapon-checkpoint.js';
import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { Weapon, Arsenal, chargeStep, applyChargeStep, CHARGE_STEPS } from '../src/weapons.js';
import { PlayerCombat } from '../src/combat.js';
import {bodyHost} from './helpers/presentation.js';
import {UnitOutput as O} from '../src/presentation/layout.js';
import { encodeEntityRecord, encodeEntityPacket, decodeEntityPacket } from '../src/runtime/network-entity-codec.js';

// A charged weapon (the Wookiee's bowcaster) on show: the simulation only
// sets the weapon's charging and held; presentation (PlayerCombat
// .presentCharge, every frame a body is drawn) shows the bank's charge
// action held over the upper body and the ChargeUpEffect at the drawn fire
// point. A world worker's page and a client online get the same two fields
// (the weapon's own state, or six bits in the soldier's record), so bots and
// other players show their charge as the player's own does.
const BOWCASTER = { class: 'all_weap_inf_bowcaster', base: 'cannon', channel: 0, ammo: 6, RoundsPerClip: 35, ReloadTime: 1.75,
  ShotDelay: 0.35, TriggerSingle: 1, MaxPressedTime: 1.75, ChargeUpEffect: 'chargeup', AnimationBank: 'rifle',
  ordnance: { base: 'bolt', Damage: 60 }, charges: [{ at: 0.15, ordnance: { base: 'bolt', Damage: 75 } }] };
const RIFLE = { class: 'rifle', base: 'cannon', channel: 0, ammo: 6, RoundsPerClip: 50, ShotDelay: 0.2, AnimationBank: 'rifle', ordnance: { base: 'bolt', Damage: 30 } };
const aim = { stick: 0, posture: 'stand', zoomed: false };
const held = { held: true, pressed: false }, released = { held: false, pressed: false };

test('the charge travels as six bits and comes back on the held weapon alone', () => {
  const arsenal = new Arsenal([BOWCASTER, RIFLE]), [bow, rifle] = arsenal.weapons;
  assert.equal(chargeStep(bow), 0, 'not charging');
  weaponCheckpoint([bow],61);
  assert.equal(bow.charging, true);
  const step = chargeStep(bow);
  assert.ok(step > 0 && step <= CHARGE_STEPS);
  assert.ok(Math.abs(step / CHARGE_STEPS - bow.held / 1.75) <= 0.5 / CHARGE_STEPS);
  // A remote copy: the step on the weapon he holds, nothing else charging.
  const copy = new Arsenal([BOWCASTER, RIFLE]);
  copy.weapons[1].charging = true;
  applyChargeStep(copy, copy.weapons[0], step);
  assert.equal(copy.weapons[0].charging, true);
  assert.ok(Math.abs(copy.weapons[0].held - bow.held) <= 1.75 / CHARGE_STEPS);
  assert.equal(copy.weapons[1].charging, false);
  applyChargeStep(copy, copy.weapons[0], 0);
  assert.equal(copy.weapons[0].charging, false);
  assert.equal(chargeStep(rifle), 0);
});

test("a soldier's record carries his charge beside the posture without growing", () => {
  const soldier = { kind: 'soldier', id: 4, generation: 2, tick: 9, classId: 1, team: 2, position: [1, 2, 3], velocity: [0, 0, 0], yaw: 0.2, pitch: 0.1,
    health: 300, posture: 1, state: 0, weapon: 3, fireCounter: 7 };
  assert.equal(encodeEntityRecord(soldier).length, 48);
  assert.equal(encodeEntityRecord({ ...soldier, charge: 63 }).length, 48);
  const decode = (record) => decodeEntityPacket(encodeEntityPacket({ epoch: 1, packetId: 1, tick: record.tick, records: [record] })).records[0];
  for (const charge of [0, 1, 37, 63]) {
    const back = decode({ ...soldier, charge });
    assert.equal(back.posture, 1);
    assert.equal(back.charge, charge);
  }
  assert.throws(() => encodeEntityRecord({ ...soldier, charge: 64 }));
  assert.throws(() => encodeEntityRecord({ ...soldier, posture: 4 }));
});

// A unit's combat as presentation sees it: the arsenal, an animator that
// logs its weapon actions, effects whose handles log their use, and a fire
// point.
function presenter(def = BOWCASTER) {
  const log = [];
  const combat = Object.create(PlayerCombat.prototype);
  const glow = { moves: 0, tint: 1, killed: false, move() { this.moves++; }, kill() { this.killed = true; } };
  Object.assign(combat, {
    arsenal: new Arsenal([def]), dead: false, mounted: false, remotes: null, soldier: { posture: 'crouch' }, glow: null,
    animator: { weaponAction: (...args) => log.push(['action', ...args]), endWeaponAction: (...args) => log.push(['end', ...args]), holdAlert: () => log.push(['alert']) },
    effects: { play: (name, position) => { log.push(['play', name, position.toArray()]); return glow; } },
    firePoint: () => ({ position: new THREE.Vector3(1, 2, 3), quaternion: new THREE.Quaternion() }),
  });
  return { combat, log, glow, weapon: combat.arsenal.weapons[0] };
}

test('a gun held at a charge keeps the alert stance and glows at the fire point', () => {
  const { combat, log, glow, weapon } = presenter();
  combat.presentCharge();
  assert.deepEqual(log, [], 'nothing while idle');
  weapon.charging = true; weapon.held = 0.875;
  combat.presentCharge(); combat.presentCharge();
  // No charge clip for a gun: the alert stance, kept up every frame.
  assert.deepEqual(log, [['alert'], ['play', 'chargeup', [1, 2, 3]], ['alert']]);
  assert.equal(glow.moves, 2, 'the glow follows the fire point every frame');
  assert.equal(glow.tint, 0.5, 'coloured by the charge');
  weapon.charging = false; weapon.held = 0;
  combat.presentCharge();
  assert.deepEqual(log.at(-1), ['end', 'charge']);
  assert.equal(glow.killed, true);
  assert.equal(combat.glow, null);
});

test("a thrown weapon's windup plays its charge clip; a charge stops showing at death, mounting or removal", () => {
  const thrown = presenter({ ...BOWCASTER, base: 'grenade', AnimationBank: 'grenade', ChargeUpEffect: undefined });
  thrown.weapon.charging = true; thrown.combat.presentCharge(); thrown.combat.presentCharge();
  assert.deepEqual(thrown.log, [['action', 'charge', 'grenade', 'crouch']], 'once, no alert');
  for (const end of ['dead', 'mounted', 'clear']) {
    const { combat, log, glow, weapon } = presenter();
    weapon.charging = true; weapon.held = 1;
    combat.presentCharge();
    if (end === 'clear') combat.clearCharge();
    else { combat[end] = true; combat.presentCharge(); }
    assert.deepEqual(log.at(-1), ['end', 'charge'], end);
    assert.equal(glow.killed, true, end);
  }
  // The Kashyyyk bowcaster has no ChargeUpEffect: the stance alone.
  const { combat, log, weapon } = presenter({ ...BOWCASTER, ChargeUpEffect: undefined });
  weapon.charging = true; combat.presentCharge();
  assert.deepEqual(log, [['alert']]);
});



test('the alert stance a charge holds comes in as after a shot and stays while it is kept up', () => {
  const root=new THREE.Group(),bone=new THREE.Bone();bone.name='DummyRoot';bone.userData.index=0;root.add(bone);
  const host=bodyHost({root,bones:[bone],byName:{DummyRoot:bone}}),animator=host.row.handle;
  const draw=dt=>{host.begin(dt);host.owner.run();return host.owner.output[O.ANIMATOR+6];};
  animator.holdAlert();assert.equal(draw(0),2.5);
  for(let i=0;i<300;i++){draw(1/60);animator.holdAlert();}
  assert.equal(draw(0),2,'a held charge sustains the timer at the phase boundary');
  animator.reset(0);draw(0);animator.fire();assert.ok(Math.abs(draw(.2)-2.3)<1e-12);
  animator.holdAlert();assert.ok(Math.abs(draw(0)-2.3)<1e-12,'a blend already under way is preserved');
  let alert;for(let i=0;i<180;i++)alert=draw(1/60);assert.ok(alert<0);

});

test("own charge correction preserves exact shared weapon continuation", async () => {
  const { Soldier } = await import('./helpers/body.js');
  const { CollisionWorld } = await import('../tools/collision-fixture.mjs');
  const { captureOwnPlayer, applyOwnPlayer } = await import('../src/runtime/network-entity-player.js');
  const { captureWeaponState } = await import('../src/runtime/network-weapon-state.js');
  const registry = { contentEpoch: 42, classes: new Map([[1, 'wookiee']]), weapons: new Map([[1, 'all_weap_inf_bowcaster'], [2, 'rifle']]), inputCodes: ['Mouse0'], bindingProfiles: new Map([[0, {}]]) };
  const plane = new CollisionWorld(new Float32Array([-100, 0, -100, 100, 0, -100, 100, 0, 100, -100, 0, 100]), new Uint32Array([0, 2, 1, 0, 3, 2]), new Uint32Array([0, 1]));
  const unit = () => {
    const soldier = new Soldier(plane, {}); soldier.spawn(0, 0, 0, 0);
    const combat = Object.create(PlayerCombat.prototype);
    Object.assign(combat, { arsenal: new Arsenal([BOWCASTER, RIFLE], { player: true, random: () => .5 }), health: 450, zoomStep: 0, firstPerson: false, soldier, kick: { rate: 0, hold: 0, offset: 0 }, dead: false, deathTime: 0 });
    return { id: 1, generation: 3, team: 1, className: 'wookiee', soldier, combat };
  };
  const capture = (u) => captureOwnPlayer({ unit: u, tick: 100, epoch: 3, registry, classId: 1, held: ['Mouse0'], view: {} });
  const source = unit(), target = unit(), bow = source.combat.arsenal.weapons[0];
  const idle = capture(source);
  weaponCheckpoint([bow],80);
  const charging = capture(source);
  assert.ok(charging.payload.length < 1100, 'complete weapon checkpoint stays bounded');
  applyOwnPlayer({ unit: target, epoch: 3, registry, record: charging });
  const shown = target.combat.arsenal.weapons[0];
  assert.equal(shown.charging, true);
  assert.equal(shown.held, bow.held);
  assert.deepEqual(captureWeaponState(shown),captureWeaponState(bow));
  const sourceShots=weaponCheckpoint([bow],1,{pressed:false,heldTicks:0});
  const shownShots=weaponCheckpoint([shown],1,{pressed:false,heldTicks:0});
  assert.deepEqual(shownShots,sourceShots);
  assert.deepEqual(captureWeaponState(shown),captureWeaponState(bow),'release keeps the exact charged cooldown');
  applyOwnPlayer({ unit: target, epoch: 3, registry, record: idle });
  assert.equal(shown.charging, false);
  assert.equal(shown.held, 0);
  assert.equal(shown.limit, 1);
});
