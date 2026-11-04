// Immutable prediction content is loaded once for each body and arsenal.
const clipNames = ['idle', 'turnl', 'turnr', 'death01', null, 'idle_to_leftfoot', null, 'walk_leftfoot_rightfoot',
  'walk_rightfoot_leftfoot', 'leftfoot_to_idle', 'rightfoot_to_idle', 'fold', 'roll', 'unfold'];
const definitionKeys = ['MaxPitchSpeed', 'BallTurnSpeed', 'BallMaxLean', 'BallAcceleration', 'BallRollingFriction'];
const flag = value => value ? 1 : 0;
export function predictionActorConfig(s, kind) {
  if (!kind) return { words: [0] };
  const clips = clipNames.map(name => name === null ? null : s.clips.get(name));
  const values = definitionKeys.flatMap(key => [flag(s.def[key] != null), s.def[key] ?? 0]);
  const words = [1, ...values];
  for (const c of clips) {
    words.push(flag(c));
    if (c) words.push(c.frames, c.fps, flag(c.rootMotion), c.tracks.length, ...c.tracks, c.data.length, ...c.data);
  }
  return { words };
}
function text(words, value) {
  words.push(value.length);
  for (let i = 0; i < value.length; i++) words.push(value.charCodeAt(i));
}
export function packPredictionWeaponDefinition(words, value, depth = 0) {
  if (value === undefined) words.push(0);
  else if (value === null) words.push(1);
  else if (value === false) words.push(2);
  else if (value === true) words.push(3);
  else if (typeof value === 'number') words.push(4, value);
  else if (typeof value === 'string') { words.push(5); text(words, value); }
  else if (Array.isArray(value) && depth < 12) {
    words.push(6, value.length);
    for (const item of value) packPredictionWeaponDefinition(words, item, depth + 1);
  } else if (typeof value === 'object' && depth < 12) {
    const keys = Object.keys(value); words.push(7, keys.length);
    for (const key of keys) { text(words, key); packPredictionWeaponDefinition(words, value[key], depth + 1); }
  } else words.push(0);
}
