// Self-contained module: Vite emits worklet URLs as assets. Keep this
// host free of relative imports so the production URL loads on its own.
function instantiateDsp(module) {
  if (!(module instanceof WebAssembly.Module)) throw new Error("audio: the Rust DSP module is missing");
  try { return new WebAssembly.Instance(module).exports; }
  catch (error) { throw new Error(`audio: the Rust DSP could not start: ${error.message}`); }
}
// The ABI exposes planar 128-frame lanes, each 512 bytes. Create views
// only during setup or after a message grows memory; process reuses them.
// A WASM memory growth detaches all views into the previous ArrayBuffer.
function views(x, pointer, count) {
  return Array.from({ length: count }, (_, i) => new Float32Array(x.memory.buffer, pointer + i * 512, 128));
}
// Pack partial room/dynamics updates with a presence mask. Missing fields
// retain Rust's current settings rather than being overwritten with zeros.
// These temporary views belong to message handling, never render work.
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
// Keep enough PCM ahead for worker scheduling jitter without retaining a
// whole track. Preroll delays scheduling until a useful window exists; chunk
// credits bound every subsequent transfer to free space in the Rust ring.
const STREAM_CAPACITY = 32768, STREAM_PREROLL = 8192, STREAM_CHUNK = 4096;
class AudioOutput extends AudioWorkletProcessor {
  constructor(options) {
    super();
    this.x = instantiateDsp(options.processorOptions.dsp);
    this.h = null;
    this.outs = null;
    this.configure(options.processorOptions);
    this.recycle = { type: "control-buffer", data: null }; this.recycleTransfer = [null];
    this.streams = []; this.poll = 0;
    this.port.onmessageerror = () => this.port.postMessage({ type: "error", message: "audio: the Rust output could not read a command; reload the game" });
    // Worklet messages and render callbacks execute serially on this thread.
    // Uploads, layout rebuilding and command-buffer growth may allocate here;
    // process below must not create arrays, views, filters or module instances.
    this.port.onmessage = ({ data: m }) => {
      try {
        const x = this.x;
        const h = this.h;
        if (m.type === "controls") this.controls(m.data);
        else if (m.type === "configure") this.configure(m.options);
        else if (m.type === "stream") this.attachStream(m);
        else if (m.type === "sample") {
          this.detachStream(m.id);
          const at = x.output_sample(h, m.id, m.frames, m.channels, m.rate);
          new Float32Array(x.memory.buffer, at, m.data.length).set(m.data);
        } else if (m.type === "release-samples") {
          for (const id of m.ids) { this.detachStream(id); x.output_free_sample(h, id); }
        } else if (m.type === "voices") {
          const at = x.output_commands_ptr(h, Math.max(m.data.length, 65536));
          new Float64Array(x.memory.buffer, at, m.data.length).set(m.data);
          x.output_commands(h, m.data.length);
        } else if (m.type === "params") {
          const at = x.output_cfg(h, Math.max(m.data.length, 8192));
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
  // Renderer.build installs the complete mix configuration before ready.
  // The Rust handle persists across layout rebuilds so live sample storage and
  // source cursors survive; layout-dependent filters are replaced between blocks.
  configure(o) {
    const x = this.x;
    if (this.h === null) this.h = x.output_new(sampleRate, o.channels, o.enhanced ? 1 : 0, o.headphones ? 1 : 0, o.binaural ? 1 : 0, o.mono ? 1 : 0, 1);
    else x.output_rebuild(this.h, o.channels, o.enhanced ? 1 : 0, o.headphones ? 1 : 0, o.binaural ? 1 : 0, o.mono ? 1 : 0);
    const h = this.h;
    this.channels = o.channels;
    this.controlLayout = o.controlLayout;
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
    // SADIE filters arrive as sixteen harmonic pairs in left/right order.
    // Copy them into staging before asking Rust to build spectra. Neither this
    // host nor the browser performs convolution or normalizes the impulses.
    if (o.filters) {
      const frames = o.filters[0].length;
      const decoderAt = x.output_decoder(h, frames);
      const dest = new Float32Array(x.memory.buffer, decoderAt, frames * 32);
      for (let i = 0; i < 32; i++) dest.set(o.filters[i], i * frames);
      x.output_decoder_ready(h, frames);
    }
    this.controlCommandsAt = x.output_commands_ptr(h, 65536); this.controlParamsAt = x.output_cfg(h, 8192); this.controlRoomAt = x.output_io(h);
    this.refresh();
    this.port.postMessage({ type: "ready", id: o.id });
  }
  // The control phase lends a reserved transfer buffer. Copy just its used
  // prefix, apply native commands and placement, then return ownership. No
  // arrays or views are constructed for an ordinary presented frame.
  controls(data) {
    const layout = this.controlLayout;
    if (!layout) throw Error("audio: missing generated control layout");
    const commands = data[layout.COMMANDS], placements = data[layout.PLACEMENTS];
    if (commands > 65536 || placements > 8192 || layout.STRIDE + commands + placements > data.length) throw Error("audio: invalid control batch");
    if (!this.outs[0].byteLength) this.refresh();
    for (let i = 0; i < commands; i++) this.controlCommands[i] = data[layout.STRIDE + i];
    for (let i = 0; i < placements; i++) this.controlParams[i] = data[layout.STRIDE + commands + i];
    const mask = data[layout.ROOM_CHANGED];
    if (mask) { for (let i = 0; i < ROOM.length; i++) this.controlRoom[i] = data[layout.ROOM + i]; this.controlRoom[ROOM.length] = mask; this.x.output_room(this.h, 0); }
    if (commands) this.x.output_commands(this.h, commands);
    if (placements) this.x.output_params(this.h, placements);
    this.recycle.data = data; this.recycleTransfer[0] = data.buffer; this.port.postMessage(this.recycle, this.recycleTransfer);
  }
  // PCM and credits travel directly between the loading worker and
  // this host. The render callback only polls preallocated records.
  // Invalidate the record before closing its producer port. An already queued
  // PCM message then cannot refill a sample ID that has been reassigned to a
  // new segment. Splicing and closing happen only outside render callbacks.
  detachStream(id) {
    for (let i = 0; i < this.streams.length; i++) if (this.streams[i].id === id) {
      this.streams[i].active = false; this.streams[i].port.close(); this.streams.splice(i, 1); return;
    }
  }
  // The worker produces decoded PCM on another thread, but this ring is owned
  // only by the worklet. Message delivery copies a bounded planar chunk into
  // Rust between blocks; there is no lock or shared cursor across threads.
  attachStream(m) {
    this.detachStream(m.id);
    const x = this.x, h = this.h, info = m.meta;
    if (info.numberOfChannels > 12 || info.length < 1) throw Error("audio: invalid streamed PCM format");
    x.output_stream_new(h, m.id, info.length, info.numberOfChannels, info.sampleRate, STREAM_CAPACITY);
    const record = { id: m.id, port: m.port, active: true, pending: false, eof: false, primed: false, buffered: 0, request: { frames: STREAM_CHUNK }, error: { type: "error", message: "audio: streamed PCM underrun; decoding could not keep up" } };
    this.streams.push(record);
    // Each reply discharges one outstanding credit request. Refresh detached
    // views after the upload, then ask for another chunk if capacity permits.
    // Primed means enough PCM has arrived to schedule, not that decoding is done.
    record.port.onmessage = ({ data: chunk }) => {
      if (!record.active) return;
      try {
        const at = x.asset_pcm(chunk.data.length);
        new Float32Array(x.memory.buffer, at, chunk.data.length).set(chunk.data);
        if (!x.output_stream_push(h, m.id, chunk.frames, Number(chunk.eof))) throw Error("audio: PCM producer exceeded its credits");
        record.pending = false; record.eof = chunk.eof; record.buffered += chunk.frames;
        if (!record.primed && (record.buffered >= STREAM_PREROLL || record.eof)) {
          record.primed = true; this.port.postMessage({ type: "primed", token: m.token });
        }
        if (!this.outs[0].byteLength) this.refresh();
        this.pullStream(record);
      } catch (error) { this.port.postMessage({ type: "error", message: error.message }); }
    };
    this.pullStream(record);
  }
  // Allow one outstanding request per producer. Reuse its message object;
  // the port snapshots it synchronously and Rust's free count is the authority
  // on ring capacity. EOF permanently stops replenishment for this segment.
  pullStream(record) {
    if (!record.pending && !record.eof && this.x.output_stream_free(this.h, record.id) >= STREAM_CHUNK) {
      record.pending = true; record.port.postMessage(record.request);
    }
  }
  // Poll every eight quanta to keep control-message overhead below sample
  // processing cost. The larger ring covers this polling interval and worker
  // jitter. An underrun is reported explicitly; render never waits for a refill.
  pollStreams() {
    if (++this.poll < 8) return;
    this.poll = 0;
    for (let i = 0; i < this.streams.length; i++) {
      const record = this.streams[i];
      if (this.x.output_stream_error(this.h, record.id)) { this.port.postMessage(record.error); throw Error(record.error.message); }
      this.pullStream(record);
    }
  }
  // Only constructors and message handlers may replace these typed views.
  // Cache the limiter report too; posting it does not require constructing a
  // new JS object inside every quantum that publishes gain reduction.
  refresh() {
    this.outs = views(this.x, this.x.output_out(this.h), this.channels);
    this.reading = new Float64Array(this.x.memory.buffer, this.x.output_io(this.h), 1);
    this.report ||= { reduction: 0 };
    this.controlCommands = new Float64Array(this.x.memory.buffer, this.controlCommandsAt, 65536);
    this.controlParams = new Float64Array(this.x.memory.buffer, this.controlParamsAt, 8192);
    this.controlRoom = new Float64Array(this.x.memory.buffer, this.controlRoomAt, ROOM.length + 1);
  }
  // Rust owns playback through the final master sample. This callback copies
  // the completed planar lanes to Web Audio's device buffer and zeros unused
  // reserved lanes. It neither mixes sources nor interprets speaker layouts.
  process(inputs, outputs) {
    this.pollStreams();
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
