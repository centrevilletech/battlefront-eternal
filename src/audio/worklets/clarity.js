// The enhanced mode's clarity stage, after the EQ and ahead of the
// dynamics (src/audio/spatial.js). Two things the game's sound lacks next
// to a 2026 mix: punch, and a top octave.
//
// - Attacks: two envelopes of the mix's power (every channel but the LFE,
//   linked, so the image holds), a fast one (1 ms up, 40 ms down) and a slow
//   one (25 ms up, 250 ms down). Where the fast one stands over the slow
//   one, an attack is under way, and the gain rises by a quarter of the
//   difference in dB past the first 1 dB, up to +4 dB, then eases back over
//   60 ms. A shot or a footstep lands harder; sustains, tails, beds and music
//   pads are left as they were, so the range between them grows.
// - A top octave: the samples are mostly 22 kHz (nothing above 11 kHz) and
//   the music and ambience 4-bit ADPCM. Per channel, the 2.5 to 7 kHz band is
//   full-wave rectified, which makes its even harmonics (an octave up and
//   more), high-passed at 7 kHz (24 dB/octave), and mixed back in at -12 dB.
//   The new top follows what is under it, sample for sample: it rises and
//   falls with each sound, in time with its attacks.
//
// processorOptions: channels, skip (the LFE's index, or -1).

const BLOCK = 128;
const SUB = 8; // samples between gain updates
const EXCITE = 10 ** (-12 / 20);
const MAX_LIFT = 10 ** (4 / 20);
const KNEE = 10 ** (1 / 10); // a power ratio of 1 dB

class Biquad {
  constructor(type, f0, q, rate) {
    const w = (2 * Math.PI * f0) / rate;
    const c = Math.cos(w);
    const alpha = Math.sin(w) / (2 * q);
    const a0 = 1 + alpha;
    if (type === "highpass") {
      this.b0 = (1 + c) / 2 / a0;
      this.b1 = -(1 + c) / a0;
      this.b2 = (1 + c) / 2 / a0;
    } else {
      this.b0 = (1 - c) / 2 / a0;
      this.b1 = (1 - c) / a0;
      this.b2 = (1 - c) / 2 / a0;
    }
    this.a1 = (-2 * c) / a0;
    this.a2 = (1 - alpha) / a0;
    this.z1 = 0;
    this.z2 = 0;
  }

  step(x) {
    const y = this.b0 * x + this.z1;
    this.z1 = this.b1 * x - this.a1 * y + this.z2;
    this.z2 = this.b2 * x - this.a2 * y;
    return y;
  }
}

class Clarity extends AudioWorkletProcessor {
  constructor(options) {
    super();
    const o = options.processorOptions || {};
    this.channels = o.channels || 2;
    this.skip = o.skip ?? -1;
    const q = Math.SQRT1_2;
    const top = Math.min(7000, sampleRate * 0.45);
    this.bands = Array.from({ length: this.channels }, () => ({
      low: new Biquad("highpass", 2500, q, sampleRate),
      high: new Biquad("lowpass", top, q, sampleRate),
      out1: new Biquad("highpass", top, q, sampleRate),
      out2: new Biquad("highpass", top, q, sampleRate),
    }));
    const coef = (s) => 1 - Math.exp(-1 / (s * sampleRate));
    this.fastUp = coef(0.001);
    this.fastDown = coef(0.04);
    this.slowUp = coef(0.025);
    this.slowDown = coef(0.25);
    this.release = coef(0.06);
    this.fast = 0;
    this.slow = 0;
    this.gain = 1;
    this.gains = new Float32Array(BLOCK);
  }

  process(inputs, outputs) {
    const input = inputs[0];
    const output = outputs[0];
    if (!input || !input.length) {
      for (const ch of output) ch.fill(0);
      return true;
    }
    const n = output[0].length;
    const C = Math.min(input.length, output.length);
    const skip = this.skip;
    const heard = Math.max(C - (skip >= 0 && skip < C ? 1 : 0), 1);
    // The mix's power per sample, a channel at a time.
    const power = this.power || (this.power = new Float64Array(BLOCK));
    power.fill(0, 0, n);
    for (let c = 0; c < C; c++) {
      if (c === skip) continue;
      const src = input[c];
      for (let i = 0; i < n; i++) {
        const x = src[i];
        power[i] += x * x;
      }
    }
    // The attack gain, per sample.
    let fast = this.fast;
    let slow = this.slow;
    let gain = this.gain;
    let goal = gain;
    const gains = this.gains;
    const { fastUp, fastDown, slowUp, slowDown, release } = this;
    for (let i = 0; i < n; i++) {
      const p = power[i] / heard;
      fast += (p > fast ? fastUp : fastDown) * (p - fast);
      slow += (p > slow ? slowUp : slowDown) * (p - slow);
      if (i % SUB === 0) {
        const ratio = (fast + 1e-12) / (slow + 1e-12);
        goal = ratio > KNEE ? Math.min((ratio / KNEE) ** 0.25, MAX_LIFT) : 1;
      }
      gain = goal > gain ? goal : gain + release * (goal - gain);
      gains[i] = gain;
    }
    this.fast = fast;
    this.slow = slow;
    this.gain = gain;
    // Each channel: the gain, and the top octave (the four filters worked in
    // place, their state in locals).
    for (let c = 0; c < output.length; c++) {
      const dst = output[c];
      const src = c < C ? input[c] : null;
      if (!src) {
        dst.fill(0);
        continue;
      }
      if (c === skip) {
        dst.set(src);
        continue;
      }
      const { low, high, out1, out2 } = this.bands[c];
      const lb0 = low.b0, lb1 = low.b1, lb2 = low.b2, la1 = low.a1, la2 = low.a2;
      const hb0 = high.b0, hb1 = high.b1, hb2 = high.b2, ha1 = high.a1, ha2 = high.a2;
      const ob0 = out1.b0, ob1 = out1.b1, ob2 = out1.b2, oa1 = out1.a1, oa2 = out1.a2;
      let l1 = low.z1, l2 = low.z2, h1 = high.z1, h2 = high.z2;
      let p1 = out1.z1, p2 = out1.z2, q1 = out2.z1, q2 = out2.z2;
      for (let i = 0; i < n; i++) {
        const x = src[i];
        const a = lb0 * x + l1;
        l1 = lb1 * x - la1 * a + l2;
        l2 = lb2 * x - la2 * a;
        const band = hb0 * a + h1;
        h1 = hb1 * a - ha1 * band + h2;
        h2 = hb2 * a - ha2 * band;
        const r = band < 0 ? -band : band;
        const e = ob0 * r + p1;
        p1 = ob1 * r - oa1 * e + p2;
        p2 = ob2 * r - oa2 * e;
        const top = ob0 * e + q1;
        q1 = ob1 * e - oa1 * top + q2;
        q2 = ob2 * e - oa2 * top;
        dst[i] = (x + EXCITE * top) * gains[i];
      }
      low.z1 = l1;
      low.z2 = l2;
      high.z1 = h1;
      high.z2 = h2;
      out1.z1 = p1;
      out1.z2 = p2;
      out2.z1 = q1;
      out2.z2 = q2;
    }
    return true;
  }
}

registerProcessor("clarity", Clarity);
