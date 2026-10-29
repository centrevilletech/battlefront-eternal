// The enhanced mode's loudness window (in the manner of "HDR audio"): the
// loudest sound playing sets the top of the window, and quieter kinds of
// sound make room under it. A blast close by pulls the ambience down by up
// to 6 dB and the foley by up to 3 dB for as long as it rings, then lets
// them back over about a second and a half; the voice-over duck stays the
// game's own (its bus attenuation). The original mode has none of this.

const WEIGHT = { explosions: 6, weapons: 3, voice: 3, foley: -3, ambience: -6, interface: 0, effects: 0 };
// How far each kind ducks, at most, and from how loud a top it starts.
const DUCK = { ambience: 6, foley: 3 };
const START = -12; // dB: a top louder than this starts the ducking
const RANGE = 12; // dB over START for the full duck
const RELEASE = 8; // dB per second the top falls back

export class LoudnessWindow {
  constructor() {
    this.top = -120;
    this.ducks = {};
  }

  update(dt, voices) {
    let loudest = -120;
    for (const v of voices) {
      if (!v.real || v.done || !(v.audible > 0)) continue;
      const level = 20 * Math.log10(v.audible) + (WEIGHT[v.category] ?? 0);
      if (level > loudest) loudest = level;
    }
    // Up at once, down slowly.
    this.top = loudest > this.top ? loudest : Math.max(loudest, this.top - RELEASE * dt);
    const depth = Math.min(Math.max((this.top - START) / RANGE, 0), 1);
    for (const [kind, db] of Object.entries(DUCK)) this.ducks[kind] = 10 ** ((-db * depth) / 20);
  }

  // The gain a voice of this kind takes now.
  gain(category) {
    return this.ducks[category] ?? 1;
  }
}
