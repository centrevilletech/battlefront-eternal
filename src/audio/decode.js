// Compressed asset I/O only. Playback and every render-quantum operation
// belong to Rust. This single boundary is replaced by the Rust FLAC/Opus
// loaders; keep its context-rate resampling contract explicit until then.
export async function decodeAudioAsset(context, bytes) {
  const rate = context.sampleRate;
  const buffer = await context.decodeAudioData(bytes);
  if (buffer.sampleRate !== rate) throw new Error(`audio: decoder returned ${buffer.sampleRate} Hz, expected ${rate} Hz`);
  return buffer;
}
