// Presentation payloads on the battle's shared module. Capture still reads
// the live objects; native writers own all byte and number serialization.
import { bridgeFor } from '../sim/runtime.js';

const NUMBERS = ['heat', 'wait', 'reloadLeft', 'throwTime', 'charge', 'held', 'lockElapsed'];
const DEFAULTS = [0, 0, 0, -1, 1, 0, 0];
const FLAGS = ['overheated', 'released', 'hidden', 'charging', 'lockComplete'];
const ERRORS = ['', 'Invalid presentation exchange', 'Unit weapon count exceeds presentation limit', 'Repair identity too long', 'Invalid unit presentation number', 'Unknown unit weapon', 'World presentation record too large'];
const number = value => typeof value === 'number' ? value : NaN;
const equal = (a, b) => a?.length === b.length && a.every((byte, i) => byte === b[i]);

export function createWirePresentation(instance) { return createWirePresentationBridge(bridgeFor(instance)); }
export function createWirePresentationBridge(bridge) {
  const x = bridge.x;
  let handle = x.wire_presentation_create();
  const unitPayloads = new WeakMap(), worldPayloads = new Map();
  function run(op, words) {
    if (!handle) throw Error('Presentation encoder disposed');
    const ptr = x.wire_presentation_room(handle, words.length);
    bridge.views(); bridge.f.set(words, ptr / 8);
    const error = x.wire_presentation_run(handle, op);
    if (error) throw Error(ERRORS[error] ?? 'Invalid presentation exchange');
    const at = x.wire_presentation_ptr(handle), size = x.wire_presentation_len(handle);
    bridge.views(); return new Uint8Array(bridge.buffer, at, size).slice();
  }
  function unit(unit, registry) {
    const arsenal = unit.combat.arsenal, weapons = arsenal.weapons;
    if (weapons.length > 12) throw Error(ERRORS[2]);
    const repair = unit.combat.repairLock ? String(unit.combat.repairLock) : '';
    const words = [unit.id, (unit.hero ? 1 : 0) | (unit.targetable ? 2 : 0) | (unit.actor?.root.visible ? 4 : 0) | (unit.combat.blowApart ? 8 : 0),
      number(unit.deadTime || 0), number(unit.soldier.stateTime || 0), unit.character ? unit.character.rosterSlot ?? 65535 : 65535,
      registry.weaponIds.get(arsenal.current(0)?.name) ?? 0, registry.weaponIds.get(arsenal.current(1)?.name) ?? 0,
      bridge.strings.id(repair), weapons.length];
    for (const w of weapons) {
      words.push(registry.weaponIds.get(w.name) ?? 0, w.levels.indexOf(w.def), FLAGS.reduce((bits, k, i) => bits | (w[k] ? 1 << i : 0), 0));
      for (let i = 0; i < NUMBERS.length; i++) words.push(number(w[NUMBERS[i]] ?? DEFAULTS[i]));
      words.push(w.lockTargetId ?? -1, w.lockGeneration ?? -1);
    }
    const bytes = run(0, words), kept = unitPayloads.get(unit);
    if (equal(kept, bytes)) return kept;
    unitPayloads.set(unit, bytes); return bytes;
  }
  function tokens(value, words) {
    if (value === null || value === undefined) words.push(0);
    else if (typeof value === 'number') words.push(1, value);
    else if (typeof value === 'boolean') words.push(2, +value);
    else if (typeof value === 'string') words.push(3, bridge.strings.id(value));
    else if (Array.isArray(value)) { words.push(4, value.length); for (const child of value) tokens(child, words); }
    else throw Error('Invalid world presentation value');
  }
  function world(value, key) {
    const words = []; tokens(value, words);
    const bytes = run(1, words), kept = worldPayloads.get(key);
    if (equal(kept, bytes)) return kept;
    if (worldPayloads.size >= 4096) worldPayloads.clear();
    worldPayloads.set(key, bytes); return bytes;
  }
  return { unit, world, dispose() { if (handle) {x.wire_presentation_drop(handle); handle = 0;} worldPayloads.clear(); } };
}
