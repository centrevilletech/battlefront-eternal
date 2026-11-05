// Self-contained module: Vite emits worklet URLs as assets. Keep this
// host free of relative imports so the production URL loads on its own.
function instantiateDsp(module) {
  if (!(module instanceof WebAssembly.Module)) throw new Error("audio: the Rust DSP module is missing");
  try { return new WebAssembly.Instance(module).exports; }
  catch (error) { throw new Error(`audio: the Rust DSP could not start: ${error.message}`); }
}
function views(x, pointer, count) {
  return Array.from({ length: count }, (_, i) => new Float32Array(x.memory.buffer, pointer + i * 512, 128));
}

// The Rust BS.1770 meter. PCM enters the module; readings only leave it.
class LoudnessMeter extends AudioWorkletProcessor {
  constructor(options) {
    super();
    const o = options.processorOptions;
    this.x = instantiateDsp(o.dsp);
    this.h = this.x.meter_new(sampleRate);
    if (o.weights) {
      const at = this.x.meter_cfg(this.h, o.weights.length);
      new Float64Array(this.x.memory.buffer, at, o.weights.length).set(o.weights);
      this.x.meter_weights(this.h, o.weights.length);
    }
    this.refresh();
    this.port.onmessage = ({ data }) => {
      if (data.reset) this.x.meter_reset(this.h);
      if (data.reserve) this.x.meter_reserve(this.h);
      this.refresh();
    };
    this.report = { momentary: 0, shortTerm: 0, integrated: 0, peak: 0, truePeak: 0, channels: new Array(o.weights.length).fill(0), reserve: false };
  }
  refresh() {
    this.ins = views(this.x, this.x.meter_in(this.h), 16);
    const at = this.x.meter_cfg(this.h, 22);
    this.reading = new Float64Array(this.x.memory.buffer, at, 22);
  }
  process(inputs) {
    const input = inputs[0];
    if (!input.length) return true;
    for (let c = 0; c < input.length; c++) this.ins[c].set(input[c]);
    if (this.x.meter_process(this.h, input.length)) {
      const a = this.reading, r = this.report;
      r.momentary = a[0]; r.shortTerm = a[1]; r.integrated = a[2]; r.peak = a[3]; r.truePeak = a[4];
      for (let c = 0; c < a[5]; c++) r.channels[c] = a[c + 6];
      r.reserve = this.x.meter_remaining(this.h) < 600;
      this.port.postMessage(r);
    }
    return true;
  }
}
registerProcessor("loudness-meter", LoudnessMeter);
