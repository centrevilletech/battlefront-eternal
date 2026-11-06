import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { Destruction } from '../src/destruction.js';
import { loadEnvironment } from '../src/environment/module.js';
import { runDestruction } from './support/destruction-scenario.js';

await loadEnvironment();

// A battle of droids and wrecks, scripted:
// droids blown apart at once and shorting out first (one a walker), a
// droideka's death blast, a blast in the world, power-up droids wrecked,
// repaired and rebuilt, the setting switched live, a moving camera. Digested
// per frame (the effects' records, the pieces' bodies as their objects show
// them, the lights, the sounds, what the units were told), held to a recorded run.
test('the destruction runs exactly as the recorded battle has it', () => {
  const golden = JSON.parse(readFileSync(new URL('./fixtures/destruction-golden.json', import.meta.url), 'utf8'));
  const rows = runDestruction(Destruction);
  assert.equal(rows.length, golden.length);
  rows.forEach((row, i) => assert.deepEqual(row, golden[i], `frame ${i}`));
});
