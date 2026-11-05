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
function keyed(x, pointer, keys, values) {
  const data = new Float64Array(x.memory.buffer, pointer, keys.length + 1);
  let mask = 0;
  for (let i = 0; i < keys.length; i++) if (values[keys[i]] !== undefined) { data[i] = values[keys[i]]; mask |= 1 << i; }
  data[keys.length] = mask;
}
const ROOM = ["room", "roomHF", "decay", "hfRatio", "reflections", "reflDelay", "reverb", "reverbDelay", "diffusion", "density", "hfReference"];
const DYNAMICS = ["threshold", "ratio", "knee", "attack", "release", "makeup", "skip"];

// One Rust-owned render quantum, with no browser inputs. Messages contain
// decoded PCM and context-timed commands; only final samples leave it.
class AudioOutput extends AudioWorkletProcessor {
  constructor(options) {
    super();
    this.x = instantiateDsp(options.processorOptions.dsp);
    this.h = null;
    this.outs = null;
    this.configure(options.processorOptions);
    this.port.onmessage = ({ data: m }) => {
      try {
        const x = this.x;
        const h = this.h;
        if (m.type === "configure") this.configure(m.options);
        else if (m.type === "sample") {
          const at = x.output_sample(h, m.id, m.frames, m.channels, m.rate);
          new Float32Array(x.memory.buffer, at, m.data.length).set(m.data);
        } else if (m.type === "release-samples") {
          for (const id of m.ids) x.output_free_sample(h, id);
        } else if (m.type === "voices") {
          const at = x.output_commands_ptr(h, m.data.length);
          new Float64Array(x.memory.buffer, at, m.data.length).set(m.data);
          x.output_commands(h, m.data.length);
        } else if (m.type === "params") {
          const at = x.output_cfg(h, m.data.length);
          new Float64Array(x.memory.buffer, at, m.data.length).set(m.data);
          x.output_params(h, m.data.length);
        } else if (m.type === "room") {
          keyed(x, x.output_io(h), ROOM, m.params); x.output_room(h, 0);
        } else if (m.type === "volume") x.output_volume(h, m.value, 0.05, m.time, 0);
        else if (m.type === "sync") this.port.postMessage({ type: "sync", id: m.id });
        if (!this.outs[0].byteLength) this.refresh();
      } catch (error) { this.port.postMessage({ type: "error", message: error.message }); throw error; }
    };
  }
  configure(o) {
    const x = this.x;
    if (this.h === null) this.h = x.output_new(sampleRate, o.channels, o.enhanced ? 1 : 0, o.headphones ? 1 : 0, o.binaural ? 1 : 0, o.mono ? 1 : 0, 1);
    else x.output_rebuild(this.h, o.channels, o.enhanced ? 1 : 0, o.headphones ? 1 : 0, o.binaural ? 1 : 0, o.mono ? 1 : 0);
    const h = this.h;
    this.channels = o.channels;
    const speakers = o.speakers;
    const decoder = o.decoder;
    const data = [];
    for (const s of speakers) data.push(...(s ? [s[0], s[1], 1] : [0, 0, 0]));
    if (decoder) data.push(decoder.rows.length, ...decoder.rows.map((r) => r ? r.length : -1), ...decoder.rows.flatMap((r) => r || []));
    const layoutAt = x.output_cfg(h, data.length);
    new Float64Array(x.memory.buffer, layoutAt, data.length).set(data);
    x.output_layout(h, o.law, speakers.length, decoder ? 1 : 0, decoder?.front || 0, o.center, o.lfe, o.original);
    for (const [kind, frequency, gain, q] of o.eq) x.output_eq(h, kind, frequency, gain, q);
    if (o.clarity) x.output_clarity(h, o.lfe, o.clarity);
    keyed(x, x.output_io(h), ROOM, o.room || {}); x.output_room(h, 1);
    keyed(x, x.output_io(h), DYNAMICS, o.dynamics); x.output_dynamics(h);
    x.output_light_level(h, o.lightLevel);
    x.output_volume(h, o.volume, 0, o.time, 1);
    if (o.filters) {
      const frames = o.filters[0].length;
      const decoderAt = x.output_decoder(h, frames);
      const dest = new Float32Array(x.memory.buffer, decoderAt, frames * 32);
      for (let i = 0; i < 32; i++) dest.set(o.filters[i], i * frames);
      x.output_decoder_ready(h, frames);
    }
    this.refresh();
    this.port.postMessage({ type: "ready", id: o.id });
  }
  refresh() {
    this.outs = views(this.x, this.x.output_out(this.h), this.channels);
    this.reading = new Float64Array(this.x.memory.buffer, this.x.output_io(this.h), 1);
    this.report ||= { reduction: 0 };
  }
  process(inputs, outputs) {
    const x = this.x;
    const report = x.output_process(this.h, currentFrame);
    for (let c = 0; c < outputs[0].length; c++) {
      if (c < this.channels) outputs[0][c].set(this.outs[c]); else outputs[0][c].fill(0);
    }
    if (report) { this.report.reduction = this.reading[0]; this.port.postMessage(this.report); }
    return true;
  }
}
registerProcessor("audio-output", AudioOutput);
