// Synthetic samples shared by native and WebAssembly combat byte twins.
import { combatRecord } from '../src/runtime/network-entity-combat.js';
import { projectileRecord } from '../src/runtime/network-combat-projectile.js';
import { feedbackRecord } from '../src/runtime/network-combat-feedback.js';
import { explosionRecord } from '../src/runtime/network-combat-explosion.js';

export const combatEncoders = { bolt: combatRecord, projectile: projectileRecord, feedback: feedbackRecord, explosion: explosionRecord };
const base = { id: 1, generation: 2, tick: 17, definitionId: 3 };
const launch = { ...base, schema: 1, expiryTick: 192, position: [1, -0, 3], direction: [0, 0, 1], life: 0.9, speed: 37.5, merge: 8 };
const projectile = { ...base, type: 'grenade', position: [1, 2, 3], life: 1 };
const feedback = { ...base, event: { kind: 'damage', position: [1, 2, 3], source: [-0, 0, 1], team: 1, attackerTeam: 2 } };

export function combatCases() {
  const cases = [], add = (encoder, value, label = '') => cases.push({ encoder, value, label: label || `${encoder} ${cases.length}` });
  let seed = 0x9f641a;
  const random = () => { seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0; return seed / 4294967296; };
  const vector = () => Array.from({ length: 3 }, () => (random() - 0.5) * 100000);
  for (let i = 0; i < 700; i++) {
    const b = { id: i + 1, generation: i % 11, tick: 1000 + i, definitionId: (i * 71) >>> 0 };
    const position = i % 9 ? vector() : [-0, Number.MIN_VALUE, -Number.MIN_VALUE];
    if (i % 7 < 2) add('bolt', { ...b, schema: i % 7 + 1, expiryTick: b.tick + 180, launchTick: b.tick - i / 8,
      executionTick: b.tick, ownerId: i * 3, ownerGeneration: i + 17, fireId: i * 5, channel: i & 1,
      position, direction: vector(), offset: vector(), speed: i * 0.15, life: i / 7, merge: i + 0.5, surface: i % 9 - 1 });
    else if (i % 7 < 4) add('feedback', { ...b, event: { kind: i % 7 === 2 ? 'damage' : 'death', position,
      target: { unit: { id: i }, generation: i % 13 }, attacker: { unit: { id: i * 2 }, generation: i % 17 },
      team: i % 3 ? i % 7 : 0xffffffff + i, attackerTeam: i % 2 ? -1 : null, source: i % 3 ? vector() : [1, NaN, 3] } });
    else if (i % 7 === 4) add('explosion', { ...b, position, wet: i & 1 });
    else add('projectile', { ...b, terminal: i % 7 === 6, type: ['grenade', 'round', 'beam'][i % 3], ownerId: i, ownerGeneration: i + 3,
      fireId: i + 10, channel: i & 1, launchTick: b.tick - 10, position, velocity: vector(), end: vector(), quaternion: [0.2, 0.4, -0.4, 0.8], life: i / 17 });
  }
  for (const value of [-1, 1.5, NaN, Infinity, 0x100000000, null, '1']) {
    for (const field of ['id', 'generation', 'tick', 'definitionId', 'expiryTick', 'executionTick']) add('bolt', { ...launch, [field]: value }, `bolt invalid ${field} ${String(value)}`);
    for (const encoder of ['explosion', 'projectile']) for (const field of ['id', 'generation', 'tick', 'definitionId']) {
      add(encoder, { ...(encoder === 'projectile' ? projectile : base), position: [1, 2, 3], [field]: value }, `${encoder} invalid ${field} ${String(value)}`);
    }
    add('feedback', { ...feedback, event: { ...feedback.event, target: { unit: { id: value }, generation: 1 } } }, `feedback identity ${String(value)}`);
  }
  for (const v of [null, new Float64Array([1, 2, 3]), [], [0, 1], [0, 1, 2, 3], [0, NaN, 0], [0, Infinity, 0], [0, undefined, 0]]) {
    for (const field of ['position', 'direction', 'offset']) add('bolt', { ...launch, [field]: v }, `bolt ${field} shape`);
    add('explosion', { ...base, position: v }, 'explosion position shape');
  }
  for (const value of [NaN, Infinity, -Infinity, 1e8, 1e8 + 1, -0, Number.MIN_VALUE]) {
    add('projectile', { ...projectile, position: [value, 0, 1] }, 'projectile scalar');
    add('feedback', { ...feedback, event: { ...feedback.event, position: [value, 0, 1] } }, 'feedback scalar');
  }
  for (const value of [-1, 2, NaN, '0', null]) {
    add('bolt', { ...launch, channel: value }, 'bolt channel');
    add('projectile', { ...projectile, channel: value }, 'projectile channel');
  }
  for (const position of [[], [1], [1, 2], [1, 2, 3, 4]]) {
    add('feedback', { ...feedback, event: { ...feedback.event, position } }, 'feedback variable vector');
    add('projectile', { ...projectile, position }, 'projectile variable vector');
  }
  add('feedback', { ...feedback, event: { kind: 'unknown' } }, 'unsupported feedback');
  add('feedback', { ...feedback, event: { ...feedback.event, source: new Array(3) } }, 'sparse feedback source');
  add('bolt', { ...launch, schema: 4, id: -1, direction: [] }, 'schema precedes vector and identity');
  add('bolt', { ...launch, id: -1, direction: [1, NaN, 3] }, 'vector scalar precedes own identity');
  add('explosion', { ...base, id: -1, position: [NaN, 0, 0] }, 'explosion identity precedes vector');
  return cases;
}
