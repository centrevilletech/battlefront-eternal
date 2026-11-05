// Asset I/O and worker scheduling only. Rust owns container parsing,
// decoding, channel order, precision, gain and sample-rate conversion.
import { AUDIO_CODEC_WASM } from "../audio-codec-wasm.js";
import { CODEC as C } from "./codec-layout.js";
const CHUNK = C.INPUT_CHUNK;
const jobs = new Map();
// One instance serves independent Rust handles. A worker may interleave
// several asset reads, but only its own event loop enters the module.
const ready = (async () => {
  const bytes = Uint8Array.from(atob(AUDIO_CODEC_WASM), (c) => c.charCodeAt(0));
  return (await WebAssembly.instantiate(bytes)).instance.exports;
})();
// Playback deadlines outrank background effect preloading. One queued
// turn admits one Rust decode phase, then yields to incoming port/I/O
// events. A slow network range never holds this scheduler's CPU turn.
const live = [], loading = [], turns = new MessageChannel();
let scheduled = false;
function schedule() {
  if (!scheduled && (live.length || loading.length)) { scheduled = true; turns.port2.postMessage(0); }
}
turns.port1.onmessage = () => {
  scheduled = false;
  const resume = live.shift() || loading.shift();
  resume?.(); schedule();
};
function turn(job) {
  return new Promise((resolve) => { (job.port ? live : loading).push(resolve); schedule(); });
}
function checked(job, status) {
  if (status === C.ERROR) {
    const x = job.x;
    throw Error(new TextDecoder().decode(new Uint8Array(x.memory.buffer, x.codec_error(job.h), x.codec_error_len(job.h))));
  }
  return status;
}
function metadata(job) {
  const x = job.x, m = new Float64Array(x.memory.buffer, x.codec_meta(job.h), C.LENGTH);
  return { sourceRate: m[C.SOURCE_RATE], sampleRate: m[C.RATE], numberOfChannels: m[C.CHANNELS], bits: m[C.BITS], length: m[C.FRAMES], format: m[C.FORMAT] };
}
function input(job, bytes, eof, tail = false) {
  if (job.abort.signal.aborted) throw Error("audio decoding cancelled");
  const x = job.x, at = x.codec_input(job.h, bytes.length);
  new Uint8Array(x.memory.buffer, at, bytes.length).set(bytes);
  checked(job, tail ? x.codec_tail(job.h) : x.codec_feed(job.h, Number(eof)));
}
// Fixed ranges avoid a file-protocol implementation eagerly retaining a
// whole compressed music response while playback is backpressured.
async function range(job, start, end) {
  const response = await fetch(job.url, { headers: { Range: `bytes=${start}-${end}` }, signal: job.abort.signal });
  if (!response.ok || /text\/html/.test(response.headers.get("content-type") || "")) throw Error(`audio asset request failed (${response.status}): ${job.url}`);
  return response;
}
async function read(job) {
  // Small already-fetched effects and HRIRs share the same parser. Their
  // input ownership transfers here once; playback never receives it.
  if (job.bytes) {
    const bytes = job.bytes.subarray(job.offset, job.offset + CHUNK);
    job.offset += bytes.length;
    input(job, bytes, job.offset === job.bytes.length);
    return;
  }
  if (job.reader) {
    // A server without byte ranges may still stream its body. Keep just
    // its current network chunk; never collect the compressed file.
    if (!job.pending?.length) {
      const next = await job.reader.read();
      if (next.done) { input(job, new Uint8Array(), true); return; }
      job.pending = next.value;
    }
    const bytes = job.pending.subarray(0, CHUNK);
    job.pending = job.pending.subarray(bytes.length);
    input(job, bytes, false); return;
  }
  if (job.total !== null && job.offset === job.total) { input(job, new Uint8Array(), true); return; }
  const response = await range(job, job.offset, job.offset + CHUNK - 1);
  if (response.status !== 206) {
    if (job.offset) throw Error("audio server stopped honoring byte ranges");
    job.reader = response.body.getReader();
    await read(job); return;
  }
  // Do not splice bytes from a changed or incorrectly served resource
  // into a codec state that has already consumed its earlier frames.
  const match = /bytes (\d+)-(\d+)\/(\d+)/.exec(response.headers.get("content-range") || "");
  if (!match || Number(match[1]) !== job.offset) throw Error("invalid audio byte range response");
  job.total = Number(match[3]);
  const bytes = new Uint8Array(await response.arrayBuffer());
  if (bytes.length !== Number(match[2]) - job.offset + 1 || bytes.length > CHUNK) throw Error("invalid audio byte range length");
  job.offset += bytes.length;
  input(job, bytes, job.offset === job.total);
}
// A step returns a complete planar transfer window, metadata, or EOF.
// NeedInput suspends only this job; the worker can serve another producer
// while its next network range arrives. No audio callback waits on I/O.
async function step(job, maximum, headers = false) {
  for (;;) {
    if (job.abort.signal.aborted) throw Error("audio decoding cancelled");
    await turn(job);
    if (job.abort.signal.aborted) throw Error("audio decoding cancelled");
    const status = checked(job, job.x.codec_step(job.h, maximum));
    if (status === C.NEED_INPUT) { await read(job); continue; }
    if (status === C.METADATA) { if (headers) return metadata(job); continue; }
    if (status === C.DONE) return { frames: 0, data: new Float32Array(), eof: true };
    const m = new Float64Array(job.x.memory.buffer, job.x.codec_meta(job.h), C.LENGTH);
    const frames = m[C.OUTPUT_FRAMES], channels = m[C.CHANNELS];
    const data = new Float32Array(job.x.memory.buffer, job.x.codec_pcm(job.h), frames * channels).slice();
    return { frames, data, eof: false };
  }
}
// Cancellation invalidates pending reads before freeing the Rust handle.
// Each resumed read checks that signal before touching module memory.
function close(job) {
  job.abort.abort(); job.reader?.cancel().catch(() => {}); job.port?.close();
  job.x.codec_free(job.h); jobs.delete(job.id);
}
async function open(m) {
  const x = await ready;
  const job = { id: m.id, x, h: x.codec_new(m.rate || 0, m.gain || 0, Number(Boolean(m.compensate))), abort: new AbortController(), url: m.url, bytes: m.bytes && new Uint8Array(m.bytes), offset: 0, total: null };
  jobs.set(m.id, job);
  try {
    let meta = await step(job, 1, true);
    // OpusHead has no track length. Read the final granule without
    // decoding the intervening packets, so the scheduler can queue the
    // next segment using its exact end time before this one has played.
    if (meta.length < 0 && meta.format === C.OPUS) {
      if (job.bytes) input(job, job.bytes.subarray(Math.max(0, job.bytes.length - CHUNK)), false, true);
      else if (job.total !== null) {
        const response = await range(job, Math.max(0, job.total - CHUNK), job.total - 1);
        if (response.status !== 206) { await response.body.cancel(); throw Error("audio duration requires byte range support"); }
        input(job, new Uint8Array(await response.arrayBuffer()), false, true);
      } else throw Error("streamed Opus requires a seekable audio asset");
      // Refresh exported metadata after installing the exact end granule.
      meta = metadata(job);
    }
    self.postMessage({ id: m.id, type: "opened", meta });
  } catch (error) { close(job); throw error; }
}
self.onmessage = async ({ data: m }) => {
  try {
    if (m.type === "open") { await open(m); return; }
    const job = jobs.get(m.id);
    if (!job) return;
    if (m.type === "close") { close(job); return; }
    // The output host owns pull credits. This port bypasses the page,
    // avoiding a second PCM queue and copies through a busy game loop.
    if (m.type === "attach") {
      job.port = m.port;
      job.port.onmessage = async ({ data: request }) => {
        try {
          const result = await step(job, request.frames);
          if (job.abort.signal.aborted) return;
          job.port.postMessage(result, [result.data.buffer]);
        } catch (error) { if (!job.abort.signal.aborted) self.postMessage({ id: job.id, type: "error", error: error.message }); }
      };
    } else if (m.type === "next") {
      const result = await step(job, m.maximum);
      self.postMessage({ id: m.id, type: "chunk", ...result }, [result.data.buffer]);
    }
  } catch (error) { self.postMessage({ id: m.id, type: "error", error: error.message }); }
};
