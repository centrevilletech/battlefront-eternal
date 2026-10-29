// The master dynamics: a compressor (the dynamics profile) and then a limiter
// that keeps the output's true peak at or under the ceiling (-1 dBTP by
// default), for any number of channels, all linked.
//
// The compressor reads the loudest channel's level (a 10 ms RMS), with a
// soft knee, and moves its gain at the attack and release times; makeup
// (dB) sets the output level after it. With ratio 1 it does nothing but the
// makeup.
//
// The limiter: Peaks are found on a 4x oversampled copy of the
// signal (BS.1770-4 Annex 2), the audio is delayed by a 1.5 ms lookahead, and
// the gain is the lookahead window's minimum, smoothed by a moving average of
// the same length (so it is down before the peak arrives) and released
// exponentially. Channels are linked: one gain for all, so the image holds.
//
// Each block is worked a stage at a time, and the costly parts only run when
// they can matter; the output is the same, sample for sample, as working
// every step on every sample:
// - No oversampled point is larger than the largest of the samples around
//   it times the interpolator's largest gain (the sum of one phase's taps'
//   magnitudes). A block whose samples stay under the ceiling over that gain
//   can't reach the ceiling, so its oversampled peaks aren't computed.
// - The window's minimum comes from a queue kept in rising order (each new
//   value drops the larger ones before it), not a search of the window.
// - Under the knee the compressor wants no gain change, which it can tell
//   from the mean square without taking its logarithm.

const PHASES = 4;
const TAPS = 12; // per phase: a 48-tap interpolator, as in BS.1770's example

function interpolator() {
  // Windowed-sinc lowpass at the original Nyquist, split into 4 phases.
  const n = PHASES * TAPS;
  const h = new Float32Array(n);
  const mid = (n - 1) / 2;
  for (let i = 0; i < n; i++) {
    const t = (i - mid) / PHASES;
    const sinc = t === 0 ? 1 : Math.sin(Math.PI * t) / (Math.PI * t);
    const w = 0.5 - 0.5 * Math.cos((2 * Math.PI * (i + 0.5)) / n);
    h[i] = sinc * w;
  }
  const phases = [];
  for (let p = 0; p < PHASES; p++) {
    const taps = new Float32Array(TAPS);
    for (let k = 0; k < TAPS; k++) taps[k] = h[k * PHASES + p];
    phases.push(taps);
  }
  return phases;
}

class TruePeakLimiter extends AudioWorkletProcessor {
  constructor(options) {
    super();
    const o = options.processorOptions || {};
    this.ceiling = 10 ** ((o.ceiling ?? -1) / 20);
    this.release = o.release ?? 0.08;
    this.look = Math.max(Math.round((o.lookahead ?? 0.0015) * sampleRate), 1);
    this.phases = interpolator();
    // The interpolator's largest gain, and at least 1 for the samples
    // themselves.
    this.reach = 1;
    for (let p = 1; p < PHASES; p++) this.reach = Math.max(this.reach, this.phases[p].reduce((s, h) => s + Math.abs(h), 0));
    this.channels = 0;
    this.history = null;
    this.delay = null;
    this.avg = new Float32Array(this.look);
    this.avgSum = this.look;
    this.avg.fill(1);
    this.pos = 0;
    this.gain = 1;
    this.minGain = 1;
    this.relCoef = 1 - Math.exp(-1 / (this.release * sampleRate));
    this.report = 0;
    // The lookahead window's wants in rising order, with the sample each
    // came in at; the front is the window's minimum.
    this.queue = new Float32Array(this.look + 1);
    this.queueAt = new Float64Array(this.look + 1);
    this.head = 0;
    this.size = 0;
    this.clock = 0;
    this.setDynamics(o.dynamics || {});
    this.compGain = 1;
    this.ms = 0;
    this.msCoef = 1 - Math.exp(-1 / (0.01 * sampleRate));
    this.block = 0;
    this.port.onmessage = (e) => {
      if (e.data.ceiling !== undefined) this.ceiling = 10 ** (e.data.ceiling / 20);
      if (e.data.release !== undefined) {
        this.release = e.data.release;
        this.relCoef = 1 - Math.exp(-1 / (this.release * sampleRate));
      }
      if (e.data.dynamics) this.setDynamics(e.data.dynamics);
    };
  }

  setDynamics(d) {
    this.threshold = d.threshold ?? 0;
    this.ratio = Math.max(d.ratio ?? 1, 1);
    this.knee = Math.max(d.knee ?? 0, 0);
    this.attackCoef = 1 - Math.exp(-1 / (Math.max(d.attack ?? 0.005, 1e-4) * sampleRate));
    this.releaseCoef = 1 - Math.exp(-1 / (Math.max(d.release ?? 0.25, 1e-3) * sampleRate));
    this.makeup = 10 ** ((d.makeup ?? 0) / 20);
    this.skip = d.skip ?? -1; // a channel the detector ignores (the LFE)
    // Mean squares under this are under the knee (with a margin far wider
    // than the logarithm's rounding), where curve() gives 0.
    this.quiet = 10 ** ((this.threshold - this.knee / 2) / 10) * (1 - 1e-9);
  }

  // The compressor's gain for a level in dB (soft knee).
  curve(level) {
    const t = this.threshold;
    const k = this.knee;
    const slope = 1 - 1 / this.ratio;
    if (this.ratio <= 1) return 0;
    if (level < t - k / 2) return 0;
    if (k > 0 && level < t + k / 2) {
      const x = level - t + k / 2;
      return (-slope * x * x) / (2 * k);
    }
    return -slope * (level - t);
  }

  setup(channels) {
    this.channels = channels;
    // Each channel's last TAPS samples, written twice (at p and p + TAPS)
    // so the interpolator reads them in one run.
    this.history = Array.from({ length: channels }, () => new Float32Array(2 * TAPS));
    this.hpos = 0;
    this.delay = Array.from({ length: channels }, () => new Float32Array(this.look));
    this.block = 0;
  }

  // Scratch for blocks of n samples.
  scratch(n) {
    if (this.block >= n) return;
    this.block = n;
    this.scaled = Array.from({ length: this.channels }, () => new Float32Array(n));
    this.sampleGain = new Float64Array(n);
    this.wants = new Float32Array(n);
    this.gains = new Float64Array(n);
  }

  process(inputs, outputs) {
    const input = inputs[0];
    const output = outputs[0];
    if (!input.length) {
      for (const c of output) c.fill(0);
      return true;
    }
    if (input.length !== this.channels) this.setup(input.length);
    const n = input[0].length;
    const C = this.channels;
    const L = this.look;
    this.scratch(n);
    const scaled = this.scaled;

    // The compressor, then the makeup: each sample's gain.
    const sampleGain = this.sampleGain;
    if (this.ratio > 1) {
      const skip = this.skip;
      const quiet = this.quiet;
      const msCoef = this.msCoef;
      let ms = this.ms;
      let compGain = this.compGain;
      for (let i = 0; i < n; i++) {
        let m = 0;
        for (let c = 0; c < C; c++) {
          if (c === skip) continue;
          const x = input[c][i];
          if (x * x > m) m = x * x;
        }
        ms += (m - ms) * msCoef;
        const want = ms + 1e-12 < quiet ? 1 : 10 ** (this.curve(10 * Math.log10(ms + 1e-12)) / 20);
        compGain += (want - compGain) * (want < compGain ? this.attackCoef : this.releaseCoef);
        sampleGain[i] = this.makeup * compGain;
      }
      this.ms = ms;
      this.compGain = compGain;
    } else {
      sampleGain.fill(this.makeup, 0, n);
    }
    // The scaled signal, and its largest sample (with the history's).
    let top = 0;
    for (let c = 0; c < C; c++) {
      const x = input[c];
      const s = scaled[c];
      const hist = this.history[c];
      for (let k = 0; k < TAPS; k++) {
        const a = Math.abs(hist[k]);
        if (a > top) top = a;
      }
      for (let i = 0; i < n; i++) {
        const v = (s[i] = x[i] * sampleGain[i]);
        const a = Math.abs(v);
        if (a > top) top = a;
      }
    }

    // What each sample wants of the limiter: 1 unless its true peak is over
    // the ceiling.
    const wants = this.wants;
    if (top * this.reach * (1 + 1e-9) <= this.ceiling) {
      // Nothing here can reach the ceiling; only the history moves on.
      wants.fill(1, 0, n);
      const from = Math.max(n - TAPS, 0);
      for (let c = 0; c < C; c++) {
        const hist = this.history[c];
        const s = scaled[c];
        for (let i = from; i < n; i++) {
          const p = (this.hpos + i) % TAPS;
          hist[p] = hist[p + TAPS] = s[i];
        }
      }
      this.hpos = (this.hpos + n) % TAPS;
    } else {
      const phases = this.phases;
      for (let i = 0; i < n; i++) {
        let peak = 0;
        const hpos = this.hpos;
        for (let c = 0; c < C; c++) {
          const hist = this.history[c];
          const v = scaled[c][i];
          hist[hpos] = hist[hpos + TAPS] = v;
          const x = Math.abs(v);
          if (x > peak) peak = x;
          for (let p = 1; p < PHASES; p++) {
            const taps = phases[p];
            let acc = 0;
            for (let k = 0; k < TAPS; k++) acc += taps[k] * hist[hpos + TAPS - k];
            const a = Math.abs(acc);
            if (a > peak) peak = a;
          }
        }
        this.hpos = (hpos + 1) % TAPS;
        wants[i] = peak > this.ceiling ? this.ceiling / peak : 1;
      }
    }

    // The gain: the window's minimum, its moving average, the release.
    const avg = this.avg;
    const queue = this.queue;
    const queueAt = this.queueAt;
    const Q = queue.length;
    const relCoef = this.relCoef;
    const gains = this.gains;
    let head = this.head;
    let size = this.size;
    let clock = this.clock;
    let avgSum = this.avgSum;
    let gain = this.gain;
    let minGain = this.minGain;
    let pos = this.pos;
    for (let i = 0; i < n; i++) {
      const w = wants[i];
      // Into the queue, past every larger want; out at the front once a
      // want has left the window.
      while (size > 0 && queue[(head + size - 1) % Q] >= w) size--;
      const back = (head + size) % Q;
      queue[back] = w;
      queueAt[back] = clock;
      size++;
      if (queueAt[head] <= clock - L) {
        head = (head + 1) % Q;
        size--;
      }
      const m = queue[head];
      clock++;
      avgSum += m - avg[pos];
      avg[pos] = m;
      const smooth = avgSum / L;
      gain = smooth < gain ? smooth : gain + (smooth - gain) * relCoef;
      if (gain < minGain) minGain = gain;
      gains[i] = gain;
      if (++pos === L) pos = 0;
    }
    // The delayed signal at that gain.
    for (let c = 0; c < C; c++) {
      const d = this.delay[c];
      const s = scaled[c];
      const out = output[c];
      let p = this.pos;
      if (out) {
        for (let i = 0; i < n; i++) {
          const delayed = d[p];
          d[p] = s[i];
          out[i] = delayed * gains[i];
          if (++p === L) p = 0;
        }
      } else {
        for (let i = 0; i < n; i++) {
          d[p] = s[i];
          if (++p === L) p = 0;
        }
      }
    }
    this.head = head;
    this.size = size;
    this.clock = clock;
    this.avgSum = avgSum;
    this.gain = gain;
    this.minGain = minGain;
    this.pos = pos;
    // Gain reduction, for the meters, about 10 times a second.
    this.report += n;
    if (this.report >= sampleRate / 10) {
      this.port.postMessage({ reduction: 20 * Math.log10(this.minGain) });
      this.minGain = 1;
      this.report = 0;
    }
    return true;
  }
}

registerProcessor("true-peak-limiter", TruePeakLimiter);
