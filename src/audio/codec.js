import { CODEC as C } from "./codec-layout.js";
// One worker owns compressed assets. Short effects can retain their PCM;
// long beds attach a bounded producer directly to a Rust output sample.
let worker;
let serial = 0;
const jobs = new Map();
function loader() {
  if (worker) return worker;
  worker = new Worker(new URL("./codec-worker.js", import.meta.url), { type: "module" });
  worker.onerror = (event) => {
    event.preventDefault();
    const error = Error(`audio: required Rust decoder failed (${event.message || "worker module could not load"})`);
    for (const job of jobs.values()) job.fail(error);
    jobs.clear(); worker.terminate(); worker = null;
  };
  worker.onmessage = ({ data: m }) => {
    const job = jobs.get(m.id);
    if (!job) return;
    if (m.type === "error") job.fail(Error(`audio: Rust asset decoding failed (${m.error})`));
    else if (m.type === "opened") { Object.assign(job, m.meta); job.duration = job.length / job.sampleRate; job.resolve(job); }
    else if (m.type === "chunk") { job.chunk?.resolve(m); job.chunk = null; }
  };
  return worker;
}
export function openAudio(input, { rate = 0, gain = 0, compensate = false } = {}) {
  return new Promise((resolve, reject) => {
    const id = ++serial;
    const job = { id, resolve, reject, onerror: null, closed: false,
      fail(error) { reject(error); this.chunk?.reject(error); this.onerror?.(error); this.close(); },
      next(maximum = C.PCM_CHUNK) {
        if (this.closed) return Promise.reject(Error("audio: decoder is closed"));
        return new Promise((resolve, reject) => { this.chunk = { resolve, reject }; worker.postMessage({ type: "next", id, maximum }); });
      },
      attach(port) { worker.postMessage({ type: "attach", id, port }, [port]); },
      close() { if (this.closed) return; this.closed = true; this.onclose?.(); jobs.delete(id); worker?.postMessage({ type: "close", id }); },
    };
    jobs.set(id, job);
    const bytes = typeof input === "string" ? null : input;
    loader().postMessage({ type: "open", id, url: bytes ? null : input, bytes, rate, gain, compensate }, bytes ? [bytes] : []);
  });
}
export class AudioPcm {
  constructor(channels, sampleRate) {
    this.channels = channels; this.sampleRate = sampleRate;
    this.length = channels[0]?.length || 0; this.numberOfChannels = channels.length; this.duration = this.length / sampleRate;
  }
  getChannelData(channel) { return this.channels[channel]; }
}
export async function loadAudioPcm(input, options) {
  const asset = await openAudio(input, options);
  try {
    const channels = Array.from({ length: asset.numberOfChannels }, () => new Float32Array(asset.length));
    let offset = 0;
    for (;;) {
      const chunk = await asset.next();
      if (chunk.eof) break;
      if (offset + chunk.frames > asset.length) throw Error("audio: decoded sample exceeds its declared length");
      for (let c = 0; c < channels.length; c++) channels[c].set(chunk.data.subarray(c * chunk.frames, (c + 1) * chunk.frames), offset);
      offset += chunk.frames;
    }
    if (offset !== asset.length) throw Error("audio: decoded sample is shorter than its declared length");
    return new AudioPcm(channels, asset.sampleRate);
  } finally { asset.close(); }
}
