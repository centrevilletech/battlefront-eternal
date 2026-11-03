// Derived from OpenAL Soft 1.20.1 (copyright the OpenAL Soft authors,
// LGPL-2.0-or-later), distributed here under GPL-3.0-only as section 3 of
// that license allows; see THIRD-PARTY-NOTICES.md.
//
// OpenAL Soft's front stabilizer, as version 1.20.1 runs it (alc/alu.cpp,
// ApplyStablizer; the Steam build's alsoft.ini turns it on, and it works on
// 5.1, 6.1 and 7.1 outputs). The original mode's last step on surround.
//
// The front pair is split at 5 kHz by a band splitter whose phase is undone
// beforehand: each block, the pair's last 256 samples and the new ones are
// run backwards through the splitter's all-pass (from rest), then forwards
// through the splitter, so the bands sum back to the input, 256 samples
// late. Then, per sample, the sum of the two lows goes a third of the way
// to the centre and the sum of the two highs a quarter of the way (as pans:
// cos and sin of 30 and 22.5 degrees); the difference stays on the pair.
//
//   L = (m + s) / 2,  R = (m - s) / 2,  C += c / 2
//   m = lows cos 30 + highs cos 22.5,  c = lows sin 30 + highs sin 22.5,
//   s = left - right
//
// Every other channel is delayed 256 samples to stay in time.

const DELAY = 256;
const BLOCK = 128;
const CROSSOVER = 5000; // Hz
const LOW_COS = Math.cos(Math.PI / 6);
const LOW_SIN = Math.sin(Math.PI / 6);
const HIGH_COS = Math.cos(Math.PI / 8);
const HIGH_SIN = Math.sin(Math.PI / 8);
// The same DSP in WebAssembly (rust/audio, ?audio=wasm):
// processorOptions.dsp is the compiled module the page hands over
// (src/audio/wasm-dsp.js). With it, blocks go to the module, whose
// answers are this file's own to the last bit
// (tests/audio-dsp-parity.mjs); without it, the JavaScript below runs as
// it always has.
const DSP_MATH = { js_sin: Math.sin, js_cos: Math.cos, js_tan: Math.tan, js_asin: Math.asin, js_atan2: Math.atan2, js_exp: Math.exp, js_pow: Math.pow, js_log10: Math.log10 };
function dspExports(options) {
  const m = options?.processorOptions?.dsp;
  if (!m) return null;
  try {
    return new WebAssembly.Instance(m, { env: DSP_MATH }).exports;
  } catch {
    return null;
  }
}


// OpenAL Soft's band splitter (alc/filters/splitter.cpp): a one-pole
// all-pass, and a low-pass of two one-pole sections sharing its coefficient;
// the high band is the all-pass less the low.
class Splitter {
  constructor(rate) {
    const w = (CROSSOVER / rate) * 2 * Math.PI;
    const cw = Math.cos(w);
    this.coeff = cw > 1.1920929e-7 ? (Math.sin(w) - 1) / cw : cw * -0.5;
    this.lp1 = 0;
    this.lp2 = 0;
    this.ap1 = 0;
  }

  // An all-pass over the first n samples of `x` in place, from rest.
  allpass(x, n) {
    const k = this.coeff;
    let z1 = 0;
    for (let i = 0; i < n; i++) {
      const y = x[i] * k + z1;
      z1 = x[i] - y * k;
      x[i] = y;
    }
  }

  // The first n samples of x into high and low; x is read from its end
  // (at - i), backwards.
  split(x, at, n, high, low) {
    const ap = this.coeff;
    const lpc = ap * 0.5 + 0.5;
    let lp1 = this.lp1;
    let lp2 = this.lp2;
    let ap1 = this.ap1;
    for (let i = 0; i < n; i++) {
      const input = x[at - i];
      let d = (input - lp1) * lpc;
      let y = lp1 + d;
      lp1 = y + d;
      d = (y - lp2) * lpc;
      y = lp2 + d;
      lp2 = y + d;
      low[i] = y;
      const a = input * ap + ap1;
      ap1 = input - a * ap;
      high[i] = a - y;
    }
    this.lp1 = lp1;
    this.lp2 = lp2;
    this.ap1 = ap1;
  }
}

class FrontStabilizer extends AudioWorkletProcessor {
  constructor(options) {
    super();
    const o = options.processorOptions || {};
    this.channels = o.channels || 6;
    this.left = o.left ?? 0;
    this.right = o.right ?? 1;
    this.centre = o.centre ?? 2;
    // Per channel: the front pair's last samples, stored backwards; the
    // others' delay line.
    this.history = Array.from({ length: this.channels }, () => new Float32Array(DELAY));
    this.ring = Array.from({ length: this.channels }, () => new Float32Array(DELAY));
    this.pos = 0;
    this.filters = [new Splitter(sampleRate), new Splitter(sampleRate)];
    this.tmp = new Float32Array(BLOCK + DELAY);
    this.bands = [0, 1].map(() => [new Float32Array(BLOCK), new Float32Array(BLOCK)]); // [high, low]
    this.silent = new Float32Array(BLOCK);
    const x = dspExports(options);
    if (x) this.dsp = { x, h: x.stabilizer_new(sampleRate, this.channels, this.left, this.right, this.centre) };
  }

  // One block through the module (128 frames, as worklets always get).
  processDsp(inputs, outputs) {
    const d = this.dsp;
    const { x, h } = d;
    const input = inputs[0] || [];
    const output = outputs[0];
    if (!d.inView || d.inView.byteLength === 0) {
      d.inView = new Float32Array(x.memory.buffer, x.stabilizer_in(h), this.channels * BLOCK);
      d.outView = new Float32Array(x.memory.buffer, x.stabilizer_out(h), this.channels * BLOCK);
    }
    const inCh = Math.min(input.length, this.channels);
    for (let c = 0; c < inCh; c++) d.inView.set(input[c], c * BLOCK);
    x.stabilizer_process(h, inCh, output.length);
    for (let c = 0; c < output.length; c++) output[c].set(d.outView.subarray(c * BLOCK, (c + 1) * BLOCK));
    return true;
  }

  // One front channel through the phase-corrected splitter. The block and
  // the history, newest first, go through the all-pass; the splitter then
  // reads them oldest first (from the end) rather than reversed in place.
  splitFront(input, side) {
    const n = input.length;
    const tmp = this.tmp;
    const history = this.history[side === 0 ? this.left : this.right];
    for (let i = 0; i < n; i++) tmp[i] = input[n - 1 - i];
    for (let i = 0; i < DELAY; i++) tmp[n + i] = history[i];
    for (let i = 0; i < DELAY; i++) history[i] = tmp[i];
    const filter = this.filters[side];
    filter.allpass(tmp, n + DELAY);
    const [high, low] = this.bands[side];
    filter.split(tmp, n + DELAY - 1, n, high, low);
  }

  process(inputs, outputs) {
    if (this.dsp && outputs[0][0]?.length === BLOCK && outputs[0].length <= this.channels) return this.processDsp(inputs, outputs);
    const input = inputs[0];
    const output = outputs[0];
    const n = output[0]?.length || BLOCK;
    const chan = (c) => (input && input[c]) || this.silent;
    // The other channels: 256 samples late.
    for (let c = 0; c < output.length; c++) {
      if (c === this.left || c === this.right) continue;
      const src = chan(c);
      const dst = output[c];
      const ring = this.ring[c];
      let p = this.pos;
      for (let i = 0; i < n; i++) {
        dst[i] = ring[p];
        ring[p] = src[i];
        if (++p === DELAY) p = 0;
      }
    }
    this.pos = (this.pos + n) % DELAY;
    this.splitFront(chan(this.left), 0);
    this.splitFront(chan(this.right), 1);
    const [lh, ll] = this.bands[0];
    const [rh, rl] = this.bands[1];
    const L = output[this.left];
    const R = output[this.right];
    const C = output[this.centre];
    for (let i = 0; i < n; i++) {
      const low = ll[i] + rl[i];
      const high = lh[i] + rh[i];
      const s = ll[i] + lh[i] - rl[i] - rh[i];
      const m = low * LOW_COS + high * HIGH_COS;
      const c = low * LOW_SIN + high * HIGH_SIN;
      L[i] = (m + s) * 0.5;
      R[i] = (m - s) * 0.5;
      if (C) C[i] += c * 0.5;
    }
    return true;
  }
}

registerProcessor("front-stabilizer", FrontStabilizer);
