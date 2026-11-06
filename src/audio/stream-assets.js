import { openAudio } from "./codec.js";
// Browser byte I/O, decoded-asset cache and worker transport only. The native
// controller owns every play gate, slot, queue and segment start/end decision.
const WHOLE_FILE = 8 << 20;
export class StreamAssets {
  constructor(engine) {
    this.engine = engine; this.files = new Map(); this.decoded = new Map();
    this.worker = null; this.jobs = new Map(); this.nextJob = 1;
  }
  setData(data) { this.data = data; }
  decode(bytes, index) {
    if (this.engine.renderer.error) return Promise.reject(this.engine.renderer.error);
    if (!this.worker) {
      this.worker = new Worker(new URL("./stream-worker.js", import.meta.url), { type: "module" });
      const fail = (message) => {
        const error = new Error(`audio: required Rust stream decoding failed: ${message}`);
        this.engine.renderer.fail(error);
        for (const job of this.jobs.values()) job.reject(error);
        this.jobs.clear();
        this.worker.terminate(); this.worker = null;
      };
      this.worker.onerror = (event) => { event.preventDefault(); fail(event.message || "worker module could not load"); };
      this.worker.onmessage = (event) => {
        if (event.data.fatal) { fail(event.data.error); return; }
        const job = this.jobs.get(event.data.id);
        this.jobs.delete(event.data.id);
        if (!job) return;
        if (event.data.error) job.reject(new Error(event.data.error));
        else job.resolve(event.data.channels);
      };
    }
    const id = this.nextJob++;
    return new Promise((resolve, reject) => {
      this.jobs.set(id, { resolve, reject });
      this.worker.postMessage(
        { id, bytes, channels: index.channels, substreams: index.substreams, interleave: index.substreamInterleave },
        [bytes],
      );
    });
  }

  async bytes(index, offset, length) {
    const url = `${this.data.base}/streams/${index.name}.bin`;
    const total = Object.values(index.segments).reduce((end, [o, s]) => Math.max(end, o + s), 0);
    if (total <= WHOLE_FILE) {
      if (!this.files.has(index.name)) this.files.set(index.name, fetch(url).then((r) => r.arrayBuffer()));
      const whole = await this.files.get(index.name);
      return whole.slice(offset, offset + length);
    }
    const response = await fetch(url, { headers: { Range: `bytes=${offset}-${offset + length - 1}` } });
    const body = await response.arrayBuffer();
    return response.status === 206 ? body : body.slice(offset, offset + length);
  }

  // A segment as an AudioBuffer: 1 or 2 channels, or 4 for a stream of two
  // stereo substreams (front pair, then rear pair).
  segment(index, segmentKey, id, variant) {
    if (variant) return openAudio(variant.url, {rate:this.engine.ctx.sampleRate,gain:variant.playback});
    if (this.decoded.has(id)) return this.decoded.get(id);
    const entry = index.segments[segmentKey];
    if (!entry) return Promise.resolve(null);
    const [offset, size, , rate] = entry;
    const count = index.substreams || 1;
    const chunk = (index.substreamInterleave || 0) * count;
    const length = count > 1 && chunk ? Math.ceil(size / chunk) * chunk : size;
    const promise = this.bytes(index, offset, length)
      .then((bytes) => this.decode(bytes, index))
      .then((channels) => {
        const ctx = this.engine.ctx;
        const frames = Math.min(...channels.map((c) => c.length));
        const buffer = ctx.createBuffer(channels.length, Math.max(frames, 1), rate);
        channels.forEach((c, i) => buffer.copyToChannel(c.subarray(0, frames), i));
        return buffer;
      });
    this.decoded.set(id, promise);
    return promise;
  }

  // Native cache policy sends the identity; deleting the promise releases
  // browser PCM when no current segment still owns it.
  evict(id) { this.decoded.delete(id); }
}
