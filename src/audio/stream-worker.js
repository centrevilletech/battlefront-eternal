// Decodes stream segments off the main thread: Xbox ADPCM bytes in, one
// Float32Array per channel out (transferred, not copied).
import { decodeSubstreams } from "./adpcm.js";

self.onmessage = (event) => {
  const { id, bytes, channels, substreams, interleave } = event.data;
  try {
    const out = decodeSubstreams(new Uint8Array(bytes), channels, substreams, interleave);
    self.postMessage({ id, channels: out }, out.map((c) => c.buffer));
  } catch (error) {
    self.postMessage({ id, error: String(error) });
  }
};
