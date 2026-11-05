// Actor prediction's class prerequisites are immutable for a body's lifetime.
// game.js replayPrediction formerly reached them through Soldier/Droideka;
// the private session instead loads them once and retains no page references.
import { ClientActorKind_INDEX, ClientValueKind_INDEX } from '../sim/gen/client.js';

const SOLDIER = ClientActorKind_INDEX.get('soldier'), DROIDEKA = ClientActorKind_INDEX.get('droideka');
const UNDEFINED = ClientValueKind_INDEX.get('undefined'), NULL = ClientValueKind_INDEX.get('null');
const FALSE = ClientValueKind_INDEX.get('false'), TRUE = ClientValueKind_INDEX.get('true');
const NUMBER = ClientValueKind_INDEX.get('number'), STRING = ClientValueKind_INDEX.get('string');
const ARRAY = ClientValueKind_INDEX.get('array'), OBJECT = ClientValueKind_INDEX.get('object');
// Droideka.clip's numeric modes have holes. Keep their places: compact
// checkpoints store the mode index, not an animation name or packed list.
const clipNames = ['idle', 'turnl', 'turnr', 'death01', null, 'idle_to_leftfoot', null, 'walk_leftfoot_rightfoot',
  'walk_rightfoot_leftfoot', 'leftfoot_to_idle', 'rightfoot_to_idle', 'fold', 'roll', 'unfold'];
const definitionKeys = ['MaxPitchSpeed', 'BallTurnSpeed', 'BallMaxLean', 'BallAcceleration', 'BallRollingFriction'];
const flag = value => value ? 1 : 0;
export function predictionActorConfig(s, kind) {
  if (kind === SOLDIER) return { words: [SOLDIER] };
  const clips = clipNames.map(name => name === null ? null : s.clips.get(name));
  // Keep a property's presence separately from its value. Droideka's class
  // defaults distinguish a missing rate from an explicitly configured zero.
  const values = definitionKeys.flatMap(key => [flag(s.def[key] != null), s.def[key] ?? 0]);
  const words = [DROIDEKA, ...values];
  for (const c of clips) {
    words.push(flag(c));
    if (c) {
      words.push(c.frames, c.fps, flag(c.rootMotion), c.tracks.length);
      for (const track of c.tracks) words.push(track);
      words.push(c.data.length);
      for (const value of c.data) words.push(value);
    }
  }
  return { words };
}
// Weapon definitions use JavaScript UTF-16 units, including surrogate pairs.
// UTF-8 encoding here would change the proven prediction definition reader.
function text(words, value) {
  words.push(value.length);
  for (let i = 0; i < value.length; i++) words.push(value.charCodeAt(i));
}
// WeaponPrediction.begin cloned the resolved weapon levels. The native loader
// needs the same undefined/null/boolean distinctions and property order; these
// tagged trees are cold content, never rebuilt while pending ticks replay.
export function packPredictionWeaponDefinition(words, value, depth = 0) {
  if (value === undefined) words.push(UNDEFINED);
  else if (value === null) words.push(NULL);
  else if (value === false) words.push(FALSE);
  else if (value === true) words.push(TRUE);
  else if (typeof value === 'number') words.push(NUMBER, value);
  else if (typeof value === 'string') { words.push(STRING); text(words, value); }
  else if (Array.isArray(value) && depth < 12) {
    words.push(ARRAY, value.length);
    for (const item of value) packPredictionWeaponDefinition(words, item, depth + 1);
  } else if (typeof value === 'object' && depth < 12) {
    const keys = Object.keys(value); words.push(OBJECT, keys.length);
    for (const key of keys) { text(words, key); packPredictionWeaponDefinition(words, value[key], depth + 1); }
  // Kept loader quirk: functions and content beyond the depth limit become
  // undefined. They are not callable hooks in the native prediction owner.
  } else words.push(UNDEFINED);
}
