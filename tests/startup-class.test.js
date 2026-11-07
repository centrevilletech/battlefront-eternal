import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { loadSim } from '../src/sim/module.js';
import { spawnPolicy } from '../src/spawn-screen/policy.js';
import { validateStartupClass } from '../src/spawn-screen/startup.js';

// A class can exist in its side's assets without being offered by a mission.
// Admission must finish before any body or local-world worker is constructed.
const mission = { sides: ['all', 'imp'], teams: {
  1: { classes: [['all_inf_pilot', 4]] },
  2: { classes: [['imp_inf_pilotatst', 4]] },
} };

test('native startup rejects an off-roster TIE pilot and admits the mission pilots', async () => {
  const instance = await loadSim();
  const policy = spawnPolicy('side=imp&unit=imp_inf_pilottie', mission);
  assert.equal(policy.team, 1, 'an unknown class otherwise defaults to team one');
  assert.throws(() => validateStartupClass(instance, policy, mission), /imp_inf_pilottie is not available in this mission/);
  for (const [side, unit] of [['imp', 'imp_inf_pilotatst'], ['all', 'all_inf_pilot']]) {
    assert.doesNotThrow(() => validateStartupClass(instance, spawnPolicy(`side=${side}&unit=${unit}`, mission), mission));
  }
  for (const team of [NaN, Infinity, -1, 0, 1.5, 3, 4294967296]) {
    assert.throws(() => validateStartupClass(instance, { className: 'all_inf_pilot', team }, mission), /not available/);
  }
});

test('startup admission accepts a long roster without truncating or changing the choice', async () => {
  const instance = await loadSim();
  const classes = Array.from({ length: 4096 }, (_, i) => [`class_${i}`, 1]);
  const large = { teams: { 1: { classes } } };
  for (const className of ['class_0', 'class_4095']) {
    assert.doesNotThrow(() => validateStartupClass(instance, { team: 1, className }, large));
  }
  instance.exports.memory.grow(1);
  assert.throws(() => validateStartupClass(instance, { team: 1, className: 'absent' }, large), /not available/);
});

const content = process.env.BFE_CONTENT_DIR || resolve('public');
const file = join(content, 'maps/end1/map.json');
test('Endor admission uses its actual pilot roster before loading the world', { skip: !existsSync(file) }, async () => {
  const data = JSON.parse(readFileSync(file));
  const actual = data.missions.end1a.mission;
  const instance = await loadSim();
  assert.throws(() => validateStartupClass(instance, spawnPolicy('side=imp&unit=imp_inf_pilottie', actual), actual), /not available/);
  for (const unit of ['imp_inf_pilotatst', 'all_inf_pilot']) {
    assert.doesNotThrow(() => validateStartupClass(instance, spawnPolicy(`unit=${unit}`, actual), actual));
  }
});
