// A loudness meter after ITU-R BS.1770 / EBU R128: K-weighting (the
// high-shelf pre-filter and the RLB high-pass, designed for the context's
// rate the way libebur128 does), channel weights (1 for front and centre,
// 1.41 for the surrounds, none for the LFE), 100 ms blocks, momentary
// (400 ms), short-term (3 s), and integrated loudness with the absolute
// (-70 LUFS) and relative (-10 LU) gates. It also reports the true peak (4x
// oversampled) and each channel's peak. Posts readings 10 times a second.
// The K-weighting design follows libebur128 (Copyright (c) 2011 Jan
// Kokemüller, MIT); see THIRD-PARTY-NOTICES.md.

function kWeighting(rate) {
  let f0 = 1681.974450955533;
  const G = 3.999843853973347;
  let Q = 0.7071752369554196;
  let K = Math.tan((Math.PI * f0) / rate);
  const Vh = 10 ** (G / 20);
  const Vb = Vh ** 0.4996667741545416;
  let a0 = 1 + K / Q + K * K;
  const pre = {
    b: [(Vh + (Vb * K) / Q + K * K) / a0, (2 * (K * K - Vh)) / a0, (Vh - (Vb * K) / Q + K * K) / a0],
    a: [(2 * (K * K - 1)) / a0, (1 - K / Q + K * K) / a0],
  };
  f0 = 38.13547087602444;
  Q = 0.5003270373238773;
  K = Math.tan((Math.PI * f0) / rate);
  a0 = 1 + K / Q + K * K;
  const rlb = { b: [1, -2, 1], a: [(2 * (K * K - 1)) / a0, (1 - K / Q + K * K) / a0] };
  return [pre, rlb];
}

const PHASES = 4;
const TAPS = 12;

class LoudnessMeter extends AudioWorkletProcessor {
  constructor(options) {
    super();
    const o = options.processorOptions || {};
    this.weights = o.weights || null; // per channel; 0 drops one (LFE)
    this.filters = kWeighting(sampleRate);
    this.block = Math.round(sampleRate / 10);
    this.channels = 0;
    this.blocks = []; // mean square of each 100 ms block (weighted sum)
    this.integrated = []; // blocks of 400 ms, for gating
    this.acc = 0;
    this.count = 0;
    this.peak = 0;
    this.truePeak = 0;
    this.chanPeak = [];
    // Interpolator for the true peak.
    const n = PHASES * TAPS;
    const h = [];
    const mid = (n - 1) / 2;
    for (let i = 0; i < n; i++) {
      const t = (i - mid) / PHASES;
      const sinc = t === 0 ? 1 : Math.sin(Math.PI * t) / (Math.PI * t);
      h.push(sinc * (0.5 - 0.5 * Math.cos((2 * Math.PI * (i + 0.5)) / n)));
    }
    this.phases = [];
    for (let p = 0; p < PHASES; p++) this.phases.push(Float32Array.from({ length: TAPS }, (_, k) => h[k * PHASES + p]));
    this.port.onmessage = (e) => {
      if (e.data.reset) {
        this.integrated = [];
        this.blocks = [];
        this.truePeak = 0;
      }
    };
  }

  setup(channels) {
    this.channels = channels;
    this.state = Array.from({ length: channels }, () => new Float64Array(8));
    // Each channel's last TAPS samples, written twice (at p and p + TAPS) so
    // the interpolator reads them in one run.
    this.hist = Array.from({ length: channels }, () => new Float32Array(2 * TAPS));
    this.hpos = 0;
    this.chanPeak = new Array(channels).fill(0);
    this.energy = null;
    if (!this.weights || this.weights.length !== channels) {
      // Defaults by count: stereo 1,1; 5.1 L R C LFE SL SR; 7.1 FL FR FC LFE BL BR SL SR.
      const w = new Array(channels).fill(1);
      if (channels >= 6) {
        w[3] = 0;
        for (let c = 4; c < channels; c++) w[c] = 1.41;
      }
      this.weights = w;
    }
  }

  process(inputs) {
    const input = inputs[0];
    if (!input.length) return true;
    if (input.length !== this.channels) this.setup(input.length);
    const n = input[0].length;
    if (!this.energy || this.energy.length < n) this.energy = new Float64Array(n);
    // A run of samples at a time, up to the end of each 100 ms block.
    let from = 0;
    while (from < n) {
      const to = Math.min(n, from + this.block - this.count);
      this.run(input, from, to);
      from = to;
      if (this.count >= this.block) this.endBlock();
    }
    return true;
  }

  // Samples from..to of every channel: each channel's K-weighted power into
  // the block's sum (in channel order, sample by sample), its peaks.
  run(input, from, to) {
    const [pre, rlb] = this.filters;
    const pb0 = pre.b[0];
    const pb1 = pre.b[1];
    const pb2 = pre.b[2];
    const pa0 = pre.a[0];
    const pa1 = pre.a[1];
    const rb0 = rlb.b[0];
    const rb1 = rlb.b[1];
    const rb2 = rlb.b[2];
    const ra0 = rlb.a[0];
    const ra1 = rlb.a[1];
    const [, p1, p2, p3] = this.phases;
    const energy = this.energy;
    energy.fill(0, from, to);
    let truePeak = this.truePeak;
    let peak = this.peak;
    const h0 = this.hpos;
    for (let c = 0; c < this.channels; c++) {
      const x = input[c];
      const s = this.state[c];
      const w = this.weights[c];
      let s0 = s[0];
      let s1 = s[1];
      let s2 = s[2];
      let s3 = s[3];
      let s4 = s[4];
      let s5 = s[5];
      let s6 = s[6];
      let s7 = s[7];
      let chanPeak = this.chanPeak[c];
      const hist = this.hist[c];
      let hpos = h0;
      for (let i = from; i < to; i++) {
        const v = x[i];
        // Two biquads, direct form I.
        const y1 = pb0 * v + pb1 * s0 + pb2 * s1 - pa0 * s2 - pa1 * s3;
        s1 = s0;
        s0 = v;
        s3 = s2;
        s2 = y1;
        const y2 = rb0 * y1 + rb1 * s4 + rb2 * s5 - ra0 * s6 - ra1 * s7;
        s5 = s4;
        s4 = y1;
        s7 = s6;
        s6 = y2;
        energy[i] += w * y2 * y2;
        const a = Math.abs(v);
        if (a > chanPeak) chanPeak = a;
        // True peak.
        hist[hpos] = hist[hpos + TAPS] = v;
        let tp = a;
        const top = hpos + TAPS;
        let acc = 0;
        for (let k = 0; k < TAPS; k++) acc += p1[k] * hist[top - k];
        if (Math.abs(acc) > tp) tp = Math.abs(acc);
        acc = 0;
        for (let k = 0; k < TAPS; k++) acc += p2[k] * hist[top - k];
        if (Math.abs(acc) > tp) tp = Math.abs(acc);
        acc = 0;
        for (let k = 0; k < TAPS; k++) acc += p3[k] * hist[top - k];
        if (Math.abs(acc) > tp) tp = Math.abs(acc);
        if (tp > truePeak) truePeak = tp;
        if (tp > peak) peak = tp;
        if (++hpos === TAPS) hpos = 0;
      }
      s[0] = s0;
      s[1] = s1;
      s[2] = s2;
      s[3] = s3;
      s[4] = s4;
      s[5] = s5;
      s[6] = s6;
      s[7] = s7;
      this.chanPeak[c] = chanPeak;
    }
    this.hpos = (h0 + (to - from)) % TAPS;
    this.truePeak = truePeak;
    this.peak = peak;
    let acc = this.acc;
    for (let i = from; i < to; i++) acc += energy[i];
    this.acc = acc;
    this.count += to - from;
  }

  endBlock() {
    this.blocks.push(this.acc / this.count);
    this.acc = 0;
    this.count = 0;
    if (this.blocks.length > 30) this.blocks.shift();
    const mean = (k) => {
      const list = this.blocks.slice(-k);
      return list.length ? list.reduce((s, x) => s + x, 0) / list.length : 0;
    };
    const lufs = (ms) => (ms > 0 ? -0.691 + 10 * Math.log10(ms) : -Infinity);
    const momentaryMs = mean(4);
    if (this.blocks.length >= 4) this.integrated.push(momentaryMs);
    // Integrated: absolute gate at -70 LUFS, then the relative gate 10 LU
    // under the mean of what passed.
    const passed = this.integrated.filter((ms) => lufs(ms) > -70);
    let integrated = -Infinity;
    if (passed.length) {
      const rel = lufs(passed.reduce((s, x) => s + x, 0) / passed.length) - 10;
      const gated = passed.filter((ms) => lufs(ms) > rel);
      if (gated.length) integrated = lufs(gated.reduce((s, x) => s + x, 0) / gated.length);
    }
    this.port.postMessage({
      momentary: lufs(momentaryMs),
      shortTerm: lufs(mean(30)),
      integrated,
      peak: this.peak > 0 ? 20 * Math.log10(this.peak) : -Infinity,
      truePeak: this.truePeak > 0 ? 20 * Math.log10(this.truePeak) : -Infinity,
      channels: this.chanPeak.map((p) => (p > 0 ? 20 * Math.log10(p) : -Infinity)),
    });
    this.peak = 0;
    this.chanPeak.fill(0);
  }
}

registerProcessor("loudness-meter", LoudnessMeter);
