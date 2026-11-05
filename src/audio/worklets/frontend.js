// Self-contained host for the Rust menu/loading render phase. PCM upload
// and commands arrive between blocks; process only renders and copies.
// Shell music uses the same bounded producer window as battle streams.
// Reserve at attachment, require preroll before scheduling, and replenish in
// 4096-frame messages rather than retaining a decoded whole track.
const STREAM_CAPACITY = 32768, STREAM_PREROLL = 8192, STREAM_CHUNK = 4096;
class FrontendOutput extends AudioWorkletProcessor {
  constructor(options) {
    super();
    const module = options.processorOptions.dsp;
    if (!(module instanceof WebAssembly.Module)) throw new Error("audio: required Rust front-end module is missing");
    this.x = new WebAssembly.Instance(module).exports;
    this.h = this.x.front_new(sampleRate);
    this.refresh();
    // Completion records and their message object are allocated before audio
    // starts. Each record includes a generation so the page can discard an end
    // notification from a previous occupant of a reused voice slot.
    this.report = { type: "ended", count: 0, voices: this.ended };
    this.streams = []; this.poll = 0;
    // This handler runs between render callbacks on the worklet thread.
    // Sample/command storage may grow here; synchronous Rust calls and copies
    // finish before process can run, so no mutex or atomic PCM state is needed.
    this.port.onmessage = ({ data: m }) => {
      try {
        const x = this.x, h = this.h;
        if (m.type === "stream") this.attachStream(m);
        else if (m.type === "sample") {
          this.detachStream(m.slot);
          const at = x.front_sample(h, m.slot, m.frames, m.channels, m.rate);
          new Float32Array(x.memory.buffer, at, m.data.length).set(m.data);
        } else if (m.type === "release") { this.detachStream(m.slot); x.front_release(h, m.slot); }
        else if (m.type === "commands") {
          const at = x.front_commands_ptr(h, m.data.length);
          new Float64Array(x.memory.buffer, at, m.data.length).set(m.data);
          x.front_commands(h);
        } else if (m.type === "sync") this.port.postMessage({ type: "sync", id: m.id });
        if (!this.left.byteLength) this.refresh();
      } catch (error) { this.port.postMessage({ type: "error", message: error.message }); throw error; }
    };
    this.port.postMessage({ type: "ready" });
  }
  // PCM and credits travel directly between the loading worker and
  // this host. The render callback only polls preallocated records.
  // Mark the old producer inactive before recycling its sample slot. Queued
  // messages from its closed port must not overwrite the next track's PCM.
  detachStream(id) {
    for (let i = 0; i < this.streams.length; i++) if (this.streams[i].id === id) {
      this.streams[i].active = false; this.streams[i].port.close(); this.streams.splice(i, 1); return;
    }
  }
  // The loading worker decodes and resamples; this host copies bounded PCM
  // windows directly into Rust's ring. Track length is scheduling metadata,
  // while physical decoded storage is capped by STREAM_CAPACITY.
  attachStream(m) {
    this.detachStream(m.id);
    const x = this.x, h = this.h, info = m.meta;
    if (info.numberOfChannels > 12 || info.length < 1) throw Error("audio: invalid streamed PCM format");
    x.front_stream_new(h, m.id, info.length, info.numberOfChannels, info.sampleRate, STREAM_CAPACITY);
    const record = { id: m.id, port: m.port, active: true, pending: false, eof: false, primed: false, buffered: 0, request: { frames: STREAM_CHUNK }, error: { type: "error", message: "audio: streamed PCM underrun; decoding could not keep up" } };
    this.streams.push(record);
    // Uploads can grow the module's memory and detach cached output/event
    // views. Repair them here, outside process, before acknowledging priming or
    // requesting the next transfer. EOF permits short tracks below preroll.
    record.port.onmessage = ({ data: chunk }) => {
      if (!record.active) return;
      try {
        const at = x.asset_pcm(chunk.data.length);
        new Float32Array(x.memory.buffer, at, chunk.data.length).set(chunk.data);
        if (!x.front_stream_push(h, m.id, chunk.frames, Number(chunk.eof))) throw Error("audio: PCM producer exceeded its credits");
        record.pending = false; record.eof = chunk.eof; record.buffered += chunk.frames;
        if (!record.primed && (record.buffered >= STREAM_PREROLL || record.eof)) {
          record.primed = true; this.port.postMessage({ type: "primed", token: m.token });
        }
        if (!this.left.byteLength) this.refresh();
        this.pullStream(record);
      } catch (error) { this.port.postMessage({ type: "error", message: error.message }); }
    };
    this.pullStream(record);
  }
  // One outstanding request prevents two replies from consuming the same
  // free-space credit. The cached request object is reused from render polling;
  // EOF closes the producer side of this refill loop.
  pullStream(record) {
    if (!record.pending && !record.eof && this.x.front_stream_free(this.h, record.id) >= STREAM_CHUNK) {
      record.pending = true; record.port.postMessage(record.request);
    }
  }
  // Polling is deliberately less frequent than rendering. Preroll and ring
  // capacity cover that interval; a genuine producer underrun is a clear error,
  // not a blocking decode call or an unreported gap in the music.
  pollStreams() {
    if (++this.poll < 8) return;
    this.poll = 0;
    for (let i = 0; i < this.streams.length; i++) {
      const record = this.streams[i];
      if (this.x.front_stream_error(this.h, record.id)) { this.port.postMessage(record.error); throw Error(record.error.message); }
      this.pullStream(record);
    }
  }
  // The ABI is stereo planar f32 plus a fixed array of slot/generation pairs.
  // Refresh only after setup or message-time memory growth, and repoint the
  // existing report at the new event view instead of replacing it per block.
  refresh() {
    const x = this.x, h = this.h, at = x.front_out(h);
    this.left = new Float32Array(x.memory.buffer, at, 128);
    this.right = new Float32Array(x.memory.buffer, at + 512, 128);
    this.ended = new Uint32Array(x.memory.buffer, x.front_ended(h), x.front_ended_len());
    if (this.report) this.report.voices = this.ended;
  }
  // The callback allocates no arrays or views and invokes no player code.
  // Rust applies source scheduling, fades and bus levels, then writes final
  // stereo PCM and zero or more completion records into existing storage.
  process(inputs, outputs) {
    this.pollStreams();
    const count = this.x.front_process(this.h, currentFrame);
    outputs[0][0].set(this.left); outputs[0][1].set(this.right);
    if (count) { this.report.count = count; this.port.postMessage(this.report); }
    return true;
  }
}
registerProcessor("audio-frontend", FrontendOutput);
