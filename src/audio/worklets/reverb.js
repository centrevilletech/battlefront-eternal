// A room reverb: a feedback delay network (Jot and Chaigne 1991) of 8 lines
// mixed by a Householder matrix, each line damped by a one-pole lowpass so
// high frequencies die faster (DecayHFRatio at HFReference), fed through
// input diffusers (allpasses, Diffusion), after early reflections
// (Reflections at ReflectionsDelay) and a pre-delay (ReverbDelay). Each
// output channel reads its own signed mix of the lines, so the tail is
// decorrelated across speakers.
//
// Parameters follow I3DL2 / EAX in linear units: room (wet level), roomHF
// (wet level at HFReference), decay (s), hfRatio, reflections, reflDelay (s),
// reverb, reverbDelay (s), diffusion (0..1), density (0..1), hfReference (Hz).
// Changes glide over about 50 ms.
//
// Every line is longer than a block, so a block's reads from the lines all
// come from earlier blocks: the network runs a stage at a time over the
// block (the input and diffusers, then each line, then the mix back in, then
// each output channel), which gives the same samples as running it one
// sample at a time. Once nothing but zeros has gone in for longer than the
// longest delay, the whole network holds zeros, and while silence keeps
// coming in, the output is silence without running it.

const LINES = 8;
// Mutually prime lengths in ms at density 1 (scaled down for lower density).
const BASE_MS = [29.7, 37.1, 41.1, 43.7, 47.3, 53.1, 59.3, 67.1];
const DIFFUSER_MS = [4.77, 3.59, 12.73, 9.31];
const EARLY_MS = [0, 3.1, 7.9, 11.3, 17.9, 23.3];
const EARLY_GAINS = [1, 0.78, 0.62, 0.55, 0.43, 0.34];
const MAX_DELAY_S = 0.4;
const KEYS = ["room", "roomHF", "decay", "hfRatio", "reflections", "reflDelay", "reverb", "reverbDelay", "diffusion", "density", "hfReference"];

class Reverb extends AudioWorkletProcessor {
  constructor(options) {
    super();
    const o = options.processorOptions || {};
    this.channels = o.channels || 2;
    const sr = sampleRate;
    this.lines = BASE_MS.map((ms) => ({ buf: new Float32Array(Math.ceil((ms / 1000) * sr * 1.6) + 2), pos: 0, len: Math.round((ms / 1000) * sr), lp: 0 }));
    this.diffusers = DIFFUSER_MS.map((ms) => ({ buf: new Float32Array(Math.round((ms / 1000) * sr) + 1), pos: 0 }));
    this.pre = { buf: new Float32Array(Math.ceil(MAX_DELAY_S * sr) + 2), pos: 0 };
    this.early = { buf: new Float32Array(Math.ceil(MAX_DELAY_S * sr) + 2), pos: 0 };
    this.earlyTaps = EARLY_MS.map((ms) => Math.round((ms / 1000) * sr));
    // Output taps: channel c reads line k with sign s[c][k] and a small
    // per-channel delay offset into the line, so channels differ.
    this.signs = [];
    for (let c = 0; c < this.channels; c++) {
      const row = [];
      for (let k = 0; k < LINES; k++) row.push(((c * 7 + k * 3 + ((c * k) % 5)) % 2 ? -1 : 1) * (1 / Math.sqrt(LINES)));
      this.signs.push(row);
    }
    this.params = { room: 0, roomHF: 1, decay: 1.5, hfRatio: 0.5, reflections: 0.3, reflDelay: 0.01, reverb: 1, reverbDelay: 0.02, diffusion: 1, density: 1, hfReference: 5000 };
    this.target = { ...this.params };
    this.set(o.params || {});
    this.params = { ...this.target };
    this.apply();
    // Samples since anything but zero went into the network, and the
    // longest delay: past it, the network holds only zeros.
    this.quiet = 0;
    this.longest = Math.max(this.pre.buf.length, this.early.buf.length, ...this.lines.map((l) => l.buf.length), ...this.diffusers.map((d) => d.buf.length));
    this.inLp = 0;
    this.block = 0;
    this.port.onmessage = (e) => {
      this.set(e.data);
    };
  }

  set(p) {
    for (const k of Object.keys(this.target)) if (p[k] !== undefined && Number.isFinite(p[k])) this.target[k] = p[k];
  }

  // Line gains and damping for the decay time and its HF ratio.
  apply() {
    const sr = sampleRate;
    const p = this.params;
    const density = Math.min(Math.max(p.density, 0), 1);
    const scale = 0.6 + 0.4 * density;
    const t60 = Math.max(p.decay, 0.1);
    const t60hf = Math.max(t60 * Math.min(Math.max(p.hfRatio, 0.1), 2), 0.05);
    const w = (2 * Math.PI * Math.min(p.hfReference, sr * 0.45)) / sr;
    for (const line of this.lines) {
      line.len = Math.max(Math.round(line.buf.length / 1.6 * scale), 8);
      const seconds = line.len / sr;
      // Gain per pass for the low and the high band.
      const g0 = 10 ** ((-3 * seconds) / t60);
      const gh = 10 ** ((-3 * seconds) / t60hf);
      // One-pole lowpass whose gain at hfReference is gh / g0.
      const r = Math.min(gh / g0, 1);
      // |H(w)| = (1 - a) / sqrt(1 - 2a cos w + a^2) = r: solve for a.
      let a = 0;
      if (r < 0.9999) {
        const c = Math.cos(w);
        const r2 = r * r;
        // (1-a)^2 = r2 (1 - 2 a c + a^2)  ->  (1 - r2) a^2 - 2 (1 - r2 c) a + (1 - r2) = 0
        const A = 1 - r2;
        const B = -2 * (1 - r2 * c);
        const disc = B * B - 4 * A * A;
        a = disc > 0 ? (-B - Math.sqrt(disc)) / (2 * A) : 0;
        a = Math.min(Math.max(a, 0), 0.99);
      }
      line.g = g0;
      line.a = a;
    }
    this.diffusion = 0.5 * Math.min(Math.max(p.diffusion, 0), 1) + 0.1;
    this.preLen = Math.min(Math.round(Math.max(p.reverbDelay, 0) * sr), this.pre.buf.length - 1);
    this.earlyLen = Math.min(Math.round(Math.max(p.reflDelay, 0) * sr), this.early.buf.length - 200);
    // The wet level's HF loss (roomHF): a one-pole on the input.
    const rh = Math.min(Math.max(p.roomHF, 0.001), 1);
    this.inA = rh >= 0.999 ? 0 : Math.min(Math.max(1 - rh, 0), 0.95);
  }

  // Scratch for blocks of n samples.
  scratch(n) {
    if (this.block >= n) return;
    this.block = n;
    this.ys = new Float64Array(n);
    this.earlies = new Float64Array(n);
    this.mixes = new Float64Array(n);
    // Each sample's line outputs, LINES per sample.
    this.values = new Float32Array(n * LINES);
  }

  process(inputs, outputs) {
    const input = inputs[0];
    const out = outputs[0];
    const n = out[0].length;
    // Glide parameters toward their targets.
    let changed = false;
    for (const k of KEYS) {
      const d = this.target[k] - this.params[k];
      if (Math.abs(d) > 1e-6) {
        this.params[k] += d * 0.1;
        changed = true;
      }
    }
    if (changed) this.apply();
    const p = this.params;
    for (const c of out) c.fill(0);
    const room = p.room;
    const inputs1 = input.length;
    // Silence in, and only zeros inside: silence out.
    if (this.quiet >= this.longest) {
      let silent = true;
      for (let i = 0; i < n && silent; i++) {
        let x = 0;
        if (inputs1) {
          for (let c = 0; c < inputs1; c++) x += input[c][i];
          x /= inputs1;
        }
        if (x * room !== 0) silent = false;
      }
      if (silent) return true;
    }
    this.scratch(n);
    const lines = this.lines;
    let shortest = Infinity;
    for (const line of lines) shortest = Math.min(shortest, line.len);
    if (shortest < n) {
      this.sampleBySample(input, out, n);
      return true;
    }

    // The input: its high cut, the early reflections, the pre-delay, and
    // the diffusers.
    const ys = this.ys;
    const earlies = this.earlies;
    const keep = 1 - this.inA;
    const e = this.early;
    const eb = e.buf;
    const elen = eb.length;
    let ep = e.pos;
    const off0 = (this.earlyLen + this.earlyTaps[0]) % elen;
    const off1 = (this.earlyLen + this.earlyTaps[1]) % elen;
    const off2 = (this.earlyLen + this.earlyTaps[2]) % elen;
    const off3 = (this.earlyLen + this.earlyTaps[3]) % elen;
    const off4 = (this.earlyLen + this.earlyTaps[4]) % elen;
    const off5 = (this.earlyLen + this.earlyTaps[5]) % elen;
    const reflect = p.reflections * 0.4;
    const pre = this.pre;
    const pb = pre.buf;
    const plen = pb.length;
    let pp = pre.pos;
    const preLen = this.preLen;
    const D = this.diffusion;
    const [d0, d1, d2, d3] = this.diffusers;
    const db0 = d0.buf;
    const db1 = d1.buf;
    const db2 = d2.buf;
    const db3 = d3.buf;
    let dp0 = d0.pos;
    let dp1 = d1.pos;
    let dp2 = d2.pos;
    let dp3 = d3.pos;
    let inLp = this.inLp;
    let stirred = false;
    for (let i = 0; i < n; i++) {
      let x = 0;
      if (inputs1) {
        for (let c = 0; c < inputs1; c++) x += input[c][i];
        x /= inputs1;
      }
      x *= room;
      inLp += (x - inLp) * keep;
      x = inLp;
      if (x !== 0) stirred = true;
      eb[ep] = x;
      let k = ep - off0;
      let early = eb[k < 0 ? k + elen : k] * EARLY_GAINS[0];
      k = ep - off1;
      early += eb[k < 0 ? k + elen : k] * EARLY_GAINS[1];
      k = ep - off2;
      early += eb[k < 0 ? k + elen : k] * EARLY_GAINS[2];
      k = ep - off3;
      early += eb[k < 0 ? k + elen : k] * EARLY_GAINS[3];
      k = ep - off4;
      early += eb[k < 0 ? k + elen : k] * EARLY_GAINS[4];
      k = ep - off5;
      early += eb[k < 0 ? k + elen : k] * EARLY_GAINS[5];
      if (++ep === elen) ep = 0;
      earlies[i] = early * reflect;
      pb[pp] = x;
      k = pp - preLen;
      let y = pb[k < 0 ? k + plen : k];
      if (++pp === plen) pp = 0;
      let delayed = db0[dp0];
      let v = y + D * delayed;
      db0[dp0] = v;
      y = delayed - D * v;
      if (++dp0 === db0.length) dp0 = 0;
      if (v !== 0) stirred = true;
      delayed = db1[dp1];
      v = y + D * delayed;
      db1[dp1] = v;
      y = delayed - D * v;
      if (++dp1 === db1.length) dp1 = 0;
      if (v !== 0) stirred = true;
      delayed = db2[dp2];
      v = y + D * delayed;
      db2[dp2] = v;
      y = delayed - D * v;
      if (++dp2 === db2.length) dp2 = 0;
      if (v !== 0) stirred = true;
      delayed = db3[dp3];
      v = y + D * delayed;
      db3[dp3] = v;
      y = delayed - D * v;
      if (++dp3 === db3.length) dp3 = 0;
      if (v !== 0) stirred = true;
      ys[i] = y;
    }
    this.inLp = inLp;
    d0.pos = dp0;
    d1.pos = dp1;
    d2.pos = dp2;
    d3.pos = dp3;
    e.pos = ep;
    pre.pos = pp;

    // Each line's damped output over the block.
    const values = this.values;
    for (let k = 0; k < LINES; k++) {
      const line = lines[k];
      const buf = line.buf;
      const len = buf.length;
      const a = line.a;
      const b = 1 - a;
      const g = line.g;
      let lp = line.lp;
      let r = line.pos - line.len;
      if (r < 0) r += len;
      for (let i = 0; i < n; i++) {
        lp = buf[r] * b + lp * a;
        values[i * LINES + k] = lp * g;
        if (++r === len) r = 0;
      }
      line.lp = lp;
      if (lp !== 0) stirred = true;
    }
    // The Householder mix (I - 2/N 11^T), fed back with the input.
    const mixes = this.mixes;
    for (let i = 0; i < n; i++) {
      const at = i * LINES;
      const sum = values[at] + values[at + 1] + values[at + 2] + values[at + 3] + values[at + 4] + values[at + 5] + values[at + 6] + values[at + 7];
      mixes[i] = (2 / LINES) * sum;
    }
    for (let k = 0; k < LINES; k++) {
      const line = lines[k];
      const buf = line.buf;
      const len = buf.length;
      let w = line.pos;
      for (let i = 0; i < n; i++) {
        const v = values[i * LINES + k] - mixes[i] + ys[i] * 0.5;
        buf[w] = v;
        if (v !== 0) stirred = true;
        if (++w === len) w = 0;
      }
      line.pos = w;
    }
    // Each speaker's own signed mix of the lines, and the early part.
    const reverb = p.reverb;
    for (let c = 0; c < out.length; c++) {
      const s = this.signs[c % this.signs.length];
      const s0 = s[0];
      const s1 = s[1];
      const s2 = s[2];
      const s3 = s[3];
      const s4 = s[4];
      const s5 = s[5];
      const s6 = s[6];
      const s7 = s[7];
      const side = c % 2 ? -0.5 : 0.5;
      const level = c < 2 ? 1 : 0.6;
      const dst = out[c];
      for (let i = 0; i < n; i++) {
        const at = i * LINES;
        const v = values[at] * s0 + values[at + 1] * s1 + values[at + 2] * s2 + values[at + 3] * s3 + values[at + 4] * s4 + values[at + 5] * s5 + values[at + 6] * s6 + values[at + 7] * s7;
        dst[i] = v * reverb + earlies[i] * side * level;
      }
    }
    this.quiet = stirred ? 0 : this.quiet + n;
    return true;
  }

  // The network one sample at a time, for lines shorter than a block (very
  // low sample rates).
  sampleBySample(input, out, n) {
    const p = this.params;
    const sr = sampleRate;
    const earlyTaps = EARLY_MS.map((ms) => Math.round((ms / 1000) * sr));
    const lines = this.lines;
    const values = new Float32Array(LINES);
    let inLp = this.inLp;
    for (let i = 0; i < n; i++) {
      let x = 0;
      if (input.length) {
        for (const ch of input) x += ch[i];
        x /= input.length;
      }
      x *= p.room;
      inLp += (x - inLp) * (1 - this.inA);
      x = inLp;
      // Early reflections.
      const e = this.early;
      e.buf[e.pos] = x;
      let early = 0;
      for (let k = 0; k < earlyTaps.length; k++) {
        const idx = (e.pos - this.earlyLen - earlyTaps[k] + e.buf.length * 2) % e.buf.length;
        early += e.buf[idx] * EARLY_GAINS[k];
      }
      e.pos = (e.pos + 1) % e.buf.length;
      early *= p.reflections * 0.4;
      // Pre-delay, then the diffusers.
      const pre = this.pre;
      pre.buf[pre.pos] = x;
      let y = pre.buf[(pre.pos - this.preLen + pre.buf.length) % pre.buf.length];
      pre.pos = (pre.pos + 1) % pre.buf.length;
      for (const d of this.diffusers) {
        const delayed = d.buf[d.pos];
        const v = y + this.diffusion * delayed;
        d.buf[d.pos] = v;
        y = delayed - this.diffusion * v;
        d.pos = (d.pos + 1) % d.buf.length;
      }
      // The network: read every line, mix by Householder (I - 2/N 11^T).
      let sum = 0;
      for (let k = 0; k < LINES; k++) {
        const line = lines[k];
        const read = line.buf[(line.pos - line.len + line.buf.length) % line.buf.length];
        line.lp = read * (1 - line.a) + line.lp * line.a;
        values[k] = line.lp * line.g;
        sum += values[k];
      }
      const h = (2 / LINES) * sum;
      for (let k = 0; k < LINES; k++) {
        const line = lines[k];
        line.buf[line.pos] = values[k] - h + y * 0.5;
        line.pos = (line.pos + 1) % line.buf.length;
      }
      for (let c = 0; c < out.length; c++) {
        const signs = this.signs[c % this.signs.length];
        let v = 0;
        for (let k = 0; k < LINES; k++) v += values[k] * signs[k];
        out[c][i] = v * p.reverb + early * (c % 2 ? -0.5 : 0.5) * (c < 2 ? 1 : 0.6);
      }
    }
    this.inLp = inLp;
    this.quiet = 0;
  }
}

registerProcessor("fdn-reverb", Reverb);
