// Direct worker ABI driver for codec goldens. Browser integration tests
// separately exercise network ranges, transferable PCM and playback.
import { AUDIO_CODEC_WASM } from "../src/audio-codec-wasm.js";
import { CODEC as C } from "../src/audio/codec-layout.js";
const module = new WebAssembly.Module(Uint8Array.from(atob(AUDIO_CODEC_WASM), (c) => c.charCodeAt(0)));
export function decodeFixture(bytes, { rate = 0, chunkSize = C.INPUT_CHUNK, maximum = 2048, gain = 0, compensate = false } = {}) {
  const x = new WebAssembly.Instance(module).exports, h = x.codec_new(rate, gain, Number(compensate));
  let offset = 0, info, planes;
  const checked = (status) => {
    if (status === C.ERROR) throw Error(new TextDecoder().decode(new Uint8Array(x.memory.buffer, x.codec_error(h), x.codec_error_len(h))));
    return status;
  };
  const feed = (data, eof, tail = false) => {
    const at = x.codec_input(h, data.length);
    new Uint8Array(x.memory.buffer, at, data.length).set(data);
    checked(tail ? x.codec_tail(h) : x.codec_feed(h, Number(eof)));
  };
  try {
    for (;;) {
      const status = checked(x.codec_step(h, maximum));
      const m = new Float64Array(x.memory.buffer, x.codec_meta(h), C.LENGTH);
      if (status === C.NEED_INPUT) {
        if (offset > bytes.length) throw Error("decoder requested bytes after EOF");
        const next = bytes.subarray(offset, offset + chunkSize);
        offset += next.length;
        feed(next, offset === bytes.length);
        if (offset === bytes.length) offset++;
      } else if (status === C.METADATA) {
        info = { sourceRate: m[C.SOURCE_RATE], rate: m[C.RATE], channels: m[C.CHANNELS], frames: m[C.FRAMES], format: m[C.FORMAT] };
        if (info.frames < 0 && info.format === C.OPUS) {
          feed(bytes.subarray(Math.max(0, bytes.length - C.INPUT_CHUNK)), false, true);
          info.frames = new Float64Array(x.memory.buffer, x.codec_meta(h), C.LENGTH)[C.FRAMES];
        }
        planes = Array.from({ length: info.channels }, () => []);
      } else if (status === C.OUTPUT) {
        const frames = m[C.OUTPUT_FRAMES];
        const pcm = new Float32Array(x.memory.buffer, x.codec_pcm(h), frames * info.channels);
        for (let c = 0; c < info.channels; c++) planes[c].push(pcm.slice(c * frames, (c + 1) * frames));
      } else if (status === C.DONE) break;
    }
    const channels = planes.map((chunks) => {
      const result = new Float32Array(chunks.reduce((n, chunk) => n + chunk.length, 0));
      let at = 0; for (const chunk of chunks) { result.set(chunk, at); at += chunk.length; }
      return result;
    });
    return { ...info, channels, heapBytes: x.memory.buffer.byteLength };
  } finally { x.codec_free(h); }
}
