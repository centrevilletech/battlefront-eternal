// How a sound's properties shape what is heard: defaults, randomization, the
// play gates, and the distance curve, as the 2004 game's sound library
// ("SoundFL") has them. docs/formats.md ("Sound") gives the addresses.

// What a SoundProperties object leaves unset. A sound is 2D unless it says
// Mode3D, and Doppler is on unless it says ModeDoppler 0.
export const DEFAULTS = {
  Pitch: 1,
  PitchDev: 0,
  Gain: 1,
  GainDev: 0,
  ReverbGain: 0,
  Looping: 0,
  Pan: 0,
  Mode3D: 0,
  ModeDoppler: 1,
  Bias: 0.5,
  Priority: 1,
  MinDistance: 0,
  MuteDistance: 1,
  MaxDistance: 1,
  RollOff: 0,
  RollIn: 0, // parsed but never used for gain
  RollInDistance: 0,
  PlayProbability: 1,
  PlayInterval: 0,
  PlayIntervalDev: 0,
  RandomPlayPos: 0,
  CyclePlayback: 0,
};

// The data's names for keys whose own names the original game doesn't spell.
export const KEY_NAMES = { "3af79d76": "RandomPlayPos", "4d5af670": "CyclePlayback" };

const clamp = (x, lo, hi) => Math.min(Math.max(x, lo), hi);

export function prop(sound, key) {
  let value = sound?.[key];
  if (value === undefined || value === null) {
    const alias = Object.keys(KEY_NAMES).find((k) => KEY_NAMES[k] === key);
    value = alias ? sound?.[alias] : undefined;
  }
  if (value === undefined || value === null) return DEFAULTS[key];
  if (key === "Gain" || key === "GainDev" || key === "ReverbGain" || key === "PlayProbability") return clamp(value, 0, 1);
  if (key === "Pan") return clamp(value, -1, 1);
  return value;
}

// A class's numeric sound setting (TurretYawSoundPitch and kin), as a
// finite number. A class with several turrets sets the key once per turret,
// so the export gives an array of strings; Number(['0.7','0.7']) is NaN,
// which an AudioParam throws on. Take the first, as sound names are taken.
export function classNumber(value, fallback) {
  const n = Number([].concat(value ?? fallback)[0]);
  return Number.isFinite(n) ? n : fallback;
}

// A uniform pick in [-1, 1): 2 (rand - 0.5).
const spread = () => 2 * (Math.random() - 0.5);

// The gain and pitch one play starts with: Gain and Pitch, each off by up to
// its deviation (added, not scaled), rolled once per play.
export function rollGain(sound) {
  return clamp(prop(sound, "Gain") + prop(sound, "GainDev") * spread(), 0, 1);
}

export function rollPitch(sound) {
  return Math.max(prop(sound, "Pitch") + prop(sound, "PitchDev") * spread(), 0.01);
}

// A sample from the SampleList: Sample(id, weight, interval, intervalDev).
// Weighted random, no repeat avoidance; with CyclePlayback the list plays in
// order from a random first pick. A sample's own interval gates it like a
// sound's PlayInterval (state kept in `gates`).
export function pickSample(list, has = () => true, state = null, now = 0) {
  const choices = (list || []).filter((entry) => has(entry[0]));
  if (!choices.length) return null;
  let entry = null;
  if (state && state.cycle !== undefined && state.cycle >= 0) {
    state.cycle = (state.cycle + 1) % choices.length;
    entry = choices[state.cycle];
  } else {
    const total = choices.reduce((sum, e) => sum + Math.max(e[1] ?? 1, 0), 0);
    let r = Math.random() * (total || choices.length);
    for (const e of choices) {
      r -= total ? Math.max(e[1] ?? 1, 0) : 1;
      if (r < 0) {
        entry = e;
        break;
      }
    }
    entry ||= choices[choices.length - 1];
    if (state && state.cycle !== undefined) state.cycle = choices.indexOf(entry);
  }
  // The sample's own minimum interval: a pick that comes too soon fails.
  if (state && (entry[2] || entry[3])) {
    const gates = (state.samples ||= new Map());
    const next = gates.get(entry[0]) ?? -Infinity;
    if (now < next) return null;
    gates.set(entry[0], now + (entry[2] || 0) + (entry[3] || 0) * spread());
  }
  return entry;
}

// The play gate: a sound asked for before its PlayInterval (+- Dev) has run
// out since it last started doesn't play; then it may not play by
// PlayProbability. The interval is set before the probability roll, so a
// failed roll still uses it up.
export function gate(sound, state, now) {
  if (now < (state.next ?? -Infinity)) return false;
  const interval = prop(sound, "PlayInterval");
  const dev = prop(sound, "PlayIntervalDev");
  if (interval || dev) state.next = now + interval + dev * spread();
  return Math.random() <= prop(sound, "PlayProbability");
}

// The gain at distance d (m), linear amplitude:
//   d < RollInDistance:      d / (rid + 10 RollOff (rid - d))   (0 at d = 0)
//   rid <= d <= MinDistance: 1
//   d >= MuteDistance:       0
//   between:                 (Max - d) / ((Max - Min) + 10 RollOff (d - Min))
// The 10 is a listener constant nothing changes.
export function distanceGain(sound, d) {
  const rid = prop(sound, "RollInDistance");
  const min = prop(sound, "MinDistance");
  const mute = prop(sound, "MuteDistance");
  const max = prop(sound, "MaxDistance");
  const roll = prop(sound, "RollOff");
  if (d < rid) return Math.max(d / (rid + 10 * roll * (rid - d)), 0);
  if (d <= min) return 1;
  if (d >= mute) return 0;
  const g = (max - d) / (max - min + 10 * roll * (d - min));
  return Math.max(Math.min(g, 1), 0);
}

// The Steam build's OpenAL output adds its own falloff on top: each source's
// reference distance is half its MaxDistance (and MinDistance is ignored), so
// past Max / 2 the level drops as (Max / 2) / d.
export function openAlFalloff(sound, d) {
  const half = 0.5 * prop(sound, "MaxDistance");
  return d > half && half > 0 ? half / d : 1;
}

// How far a sound carries: past this it can't be heard at all.
export function reach(sound) {
  return prop(sound, "MuteDistance");
}
