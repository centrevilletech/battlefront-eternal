import { contentPath } from './helpers/content.js';
// The Dark Trooper's blast cannon and the Rebel pilot's blaster cannon
// (imp_/all_weap_inf_shotgun): five bolts a press, each drawing its own
// spread, and the page's predicted copies flying where the authority's do.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import { Arsenal } from '../src/weapons.js';
import { PlayerCombat } from '../src/combat.js';
import { prepareMelee } from '../src/heroes.js';
import { PredictionArsenal } from '../src/runtime/prediction-arsenal.js';
import { createClientPrediction } from '../src/sim/wasm/runtime/client-prediction.js';
import { loadSim } from '../src/sim/module.js';
import { loadRayKernel } from '../src/sim/wasm/ray-kernel.js';
import { actorCourse, createPredictionActor } from './fixtures/client-actors.js';

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
test('the Rust page session emits five independently seeded pellets for one press', async () => {
  const instance = await loadSim(), course = actorCourse(await loadRayKernel(instance));
  const body = createPredictionActor({kind:'soldier'}, course);
  const combat = body.combat;
  combat.arsenal = prepareMelee(new Arsenal([blastCannon()], { player: true })); combat.zoomStep=0;
  const unit = { id: 6, generation: 2, alive: true, soldier: body.soldier, combat };
  const prediction = new PredictionArsenal(), session = createClientPrediction(instance, course.world), cosmetic = [];
  try {
    prediction.begin(unit, { tick: 0 });
    session.predict(unit, [{tick:1,sequence:1,generation:2,actions:['firePrimary'],down:['Mouse0'],look:[]}], {
      epoch:1,tick:0,fireEdges:[{kind:'fire',sequence:1,button:'firePrimary',pressed:true,id:41}],
    });
    session.present(unit,null,notice=>cosmetic.push({yaw:notice.yaw,pitch:notice.pitch}));
  } finally {prediction.dispose();session.dispose();course.dispose();}
  assert.equal(cosmetic.length,5);
  assert.equal(new Set(cosmetic.map(s=>s.yaw)).size,5);
  assert.ok(cosmetic.every(s=>Math.abs(s.yaw)<3*DEG&&Math.abs(s.pitch)<1.5*DEG));
});

for (const [side, unit] of [['imp', 'imp_inf_dark_trooper'], ['imp', 'imp_inf_dark_troopersnow'], ['all', 'all_inf_pilot']]) {
  const dir = process.env.BFE_TEST_CONTENT_DIR || contentPath();
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
  });
}
