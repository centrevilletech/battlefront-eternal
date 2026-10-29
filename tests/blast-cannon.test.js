// The Dark Trooper's blast cannon and the Rebel pilot's blaster cannon
// (imp_/all_weap_inf_shotgun): five bolts a press, each drawing its own
// spread, and the page's predicted copies flying where the authority's do.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import { Arsenal, Weapon, seededRandom, spreadSeed } from '../src/weapons.js';
import { PlayerCombat } from '../src/combat.js';
import { prepareMelee } from '../src/heroes.js';
import { readWeaponInput, seedPresses } from '../src/combat-weapon-input.js';
import { networkShotFor } from '../src/runtime/network-shot-aim.js';
import { WeaponPrediction } from '../src/runtime/weapon-prediction.js';
import { CommandControls } from '../src/runtime/commands.js';

const DEG = Math.PI / 180;
// The retail class as the extractor exports it (SIDE/imp.lvl).
const blastCannon = (extra = {}) => ({
  class: 'imp_weap_inf_shotgun', base: 'cannon', channel: 0, ammo: 7, RoundsPerClip: 30, ReloadTime: 1.75,
  ShotDelay: 0.75, TriggerSingle: 1, SalvoCount: 1, ShotsPerSalvo: 5, SalvoDelay: 0, InitialSalvoDelay: 0,
  PitchSpread: 1.5, YawSpread: 3, SpreadPerShot: 1.1, SpreadRecoverRate: 4.8, SpreadThreshold: 1.6, SpreadLimit: 6,
  StandStillSpread: 0, StandMoveSpread: 0, ZoomMin: 2, ZoomMax: 2,
  ordnance: { class: 'imp_weap_inf_shotgun_ord', base: 'bolt', Velocity: 150, LifeSpan: 0.8, Damage: 55 },
  ...extra,
});
const aim = { stick: 0, posture: 'stand', zoomed: false };
const press = (seed = null) => ({ held: true, pressed: true, ...(seed == null ? {} : { seed }) });
const angles = (shots) => shots.map((s) => [s.yaw, s.pitch]);

test('a press fires five rounds, each with its own spread, spending five rounds of the clip', () => {
  const gun = new Weapon(blastCannon());
  const shots = gun.update(1 / 60, true, press(spreadSeed({ id: 3, generation: 1 }, 0, 9)), false, aim);
  assert.equal(shots.length, 5);
  assert.equal(gun.loaded, 25);
  assert.equal(new Set(shots.map((s) => s.yaw)).size, 5, 'every round draws its own yaw');
  assert.equal(new Set(shots.map((s) => s.pitch)).size, 5, 'and its own pitch');
  for (const s of shots) {
    assert.ok(Math.abs(s.yaw) < 3 * DEG && Math.abs(s.pitch) < 1.5 * DEG, 'inside YawSpread and PitchSpread');
  }
  // The kick grows once per salvo, not once per round.
  assert.ok(Math.abs(gun.kick - (-1.6 + 1.1)) < 1e-9);
  // TriggerSingle: holding the trigger fires nothing more; the next press
  // waits out ShotDelay.
  assert.equal(gun.update(0.5, true, { held: true, pressed: false }, false, aim).length, 0);
  assert.equal(gun.update(0.1, true, press(), false, aim).length, 0);
  assert.equal(gun.update(0.2, true, press(), false, aim).length, 5);
});

test('rounds fan out across the whole spread over many presses', () => {
  const gun = new Weapon(blastCannon({ ammo: 0, RoundsPerClip: 0 }));
  let wide = 0, low = 0, n = 0;
  for (let p = 1; p <= 400; p++) {
    for (const s of gun.update(1, true, press(spreadSeed({ id: 1, generation: 1 }, 0, p)), false, aim)) {
      n++;
      if (Math.abs(s.yaw) > 1.5 * DEG) wide++;
      if (Math.abs(s.pitch) < 0.75 * DEG) low++;
    }
  }
  assert.equal(n, 2000);
  // (U1 - U2) x S is triangular: a quarter of the rounds land past half of
  // S, three quarters inside it.
  assert.ok(Math.abs(wide / n - 0.25) < 0.04, `wide share ${wide / n}`);
  assert.ok(Math.abs(low / n - 0.75) < 0.04, `narrow share ${low / n}`);
});

test('the same press draws the same salvo on the authority and in prediction', () => {
  const seed = spreadSeed({ id: 12, generation: 4 }, 0, 77);
  const authority = new Weapon(blastCannon());
  const predicted = new Weapon(blastCannon(), { random: () => 0.5 });
  const a = authority.update(1 / 60, true, press(seed), false, aim);
  const b = predicted.update(1 / 60, true, press(seed), false, aim);
  assert.deepEqual(angles(a), angles(b));
  // Without a press's seed the prediction keeps the middle of the spread, as
  // before, and the authority its own draws.
  const unseeded = new Weapon(blastCannon(), { random: () => 0.5 });
  assert.deepEqual(angles(unseeded.update(1 / 60, true, press(), false, aim)), Array(5).fill([0, 0]));
  // Another press, life or channel draws another salvo.
  for (const other of [spreadSeed({ id: 12, generation: 4 }, 0, 78), spreadSeed({ id: 12, generation: 5 }, 0, 77), spreadSeed({ id: 13, generation: 4 }, 0, 77), spreadSeed({ id: 12, generation: 4 }, 1, 77)]) {
    assert.notEqual(other, seed);
    const gun = new Weapon(blastCannon());
    assert.notDeepEqual(angles(gun.update(1 / 60, true, press(other), false, aim)), angles(a));
  }
  // The generator is integer arithmetic: the same numbers everywhere.
  const r = seededRandom(1);
  assert.deepEqual([r(), r(), r()].map((x) => x.toFixed(12)), ['0.627073940588', '0.002735721180', '0.527447039960']);
});

test('the authority seeds a press from its trigger edge and the page from its own press id', () => {
  const owner = { id: 6, generation: 2, networkShots: [{ id: 41, button: 0, pressed: true, aimYaw: 0, aimPitch: 0 }] };
  const controls = new CommandControls();
  controls.down = new Set(['Mouse0']);
  controls.queued.add('firePrimary');
  const input = seedPresses(readWeaponInput(controls), owner, [0, 1].map((channel) => networkShotFor(owner, channel)?.id));
  assert.equal(input.primary.seed, spreadSeed(owner, 0, 41));
  assert.equal(input.secondary.seed, undefined, 'no press, no seed');
  // A held trigger without a press takes no seed.
  const held = new CommandControls();
  held.down = new Set(['Mouse0']);
  assert.equal(seedPresses(readWeaponInput(held), owner, [41, null]).primary.seed, undefined);
  // A bot has no trigger edges: its weapons keep the AI generator.
  assert.equal(seedPresses(readWeaponInput(controls), { id: 7, generation: 1 }, [null, null]).primary.seed, undefined);

  // The authority's salvo, fired through Arsenal.update as PlayerCombat does.
  const authority = new Arsenal([blastCannon()], { player: true });
  const fired = authority.update(1 / 60, input, aim);
  // The page's: WeaponPrediction replays the same press on its private copy.
  const soldier = { posture: 'stand', aimYaw: 0, aimPitch: 0 };
  const combat = Object.create(PlayerCombat.prototype);
  Object.assign(combat, { arsenal: prepareMelee(new Arsenal([blastCannon()], { player: true })), soldier, zoomStep: 0, dead: false });
  const unit = { id: 6, generation: 2, alive: true, soldier, combat };
  const prediction = new WeaponPrediction();
  prediction.begin(unit, { tick: 0 });
  const cosmetic = [];
  prediction.step({ actions: ['firePrimary'], down: ['Mouse0'] }, 1, (shot) => cosmetic.push(shot), [41, null]);
  prediction.restore();
  assert.equal(fired.length, 5);
  assert.deepEqual(angles(cosmetic), angles(fired), 'each predicted pellet flies the authority pellet\'s path');
});

for (const [side, unit] of [['imp', 'imp_inf_dark_trooper'], ['imp', 'imp_inf_dark_troopersnow'], ['all', 'all_inf_pilot']]) {
  const dir = process.env.BFE_TEST_CONTENT_DIR || 'public';
  const file = `${dir}/sides/${side}/units.json`;
  test(`retail ${unit} carries the five-bolt cannon`, { skip: !fs.existsSync(file) }, () => {
    const def = JSON.parse(fs.readFileSync(file)).units[unit]?.weapons?.find((w) => w.class === `${side}_weap_inf_shotgun`);
    assert.ok(def, 'the shotgun is in the loadout');
    assert.equal(def.ShotsPerSalvo, 5);
    assert.equal(def.SalvoCount, 1);
    assert.equal(def.TriggerSingle, 1);
    assert.equal(def.ordnance.base, 'bolt');
    // MaxDamage in the class, the same field as Damage.
    assert.equal(def.ordnance.Damage, 55);
    assert.equal(def.ordnance.Velocity * def.ordnance.LifeSpan, 120, 'a bolt reaches 120 m');
    // One way to fire (the 2004 game reads no ModeTexture or
    // ChangeModeSound): no charge levels, and the zoom key's 2x.
    assert.equal((def.charges || []).length, 0);
    assert.deepEqual(PlayerCombat.zooms(def), [1, 2]);
    const gun = new Weapon(def);
    assert.equal(gun.update(1 / 60, true, press(spreadSeed({ id: 1, generation: 1 }, 0, 1)), false, aim).length, 5);
  });
}
