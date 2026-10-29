import test from 'node:test';
import assert from 'node:assert/strict';
import { flightAcceptance } from './flight-acceptance.js';

test('flight acceptance: takeoff, maneuvering, collision, landing, targeting and frame-rate parity', () => {
  const results = flightAcceptance();
  assert.equal(results.length,14);
  for (const result of results) assert.equal(result.status,'pass',`${result.name}: ${JSON.stringify(result.detail)}`);
});
