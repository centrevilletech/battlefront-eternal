// Synthetic menu/loading sounds. Native reference waveforms were recorded
// before replacing those graphs; this file contains inputs, never DSP.
import { FrontendAudio } from "../src/audio/frontend.js";
export function frontendSpec(scene) {
  const loading = scene.loading;
  return {
    rate: scene.rate, seconds: 0.6, master: 0.8, music: 0.42, fx: 0.15,
    voices: loading ? [
      { channels: 1, rate: 8000, frames: 2800, pitch: 1.8, loop: true, gain: 0.7, when: 0.013, offset: 0.027, fadeAt: 0.31, fadeEnd: 0.32 },
      { channels: 1, rate: 22050, frames: 3300, pitch: 1.5, gain: 1, when: 0.12 },
      { channels: 1, rate: 8000, frames: 2800, pitch: 2.1, loop: true, gain: 0.7, when: 0.29, fadeAt: 0.48, fadeEnd: 0.49 },
    ] : [
      { channels: 2, rate: scene.rate, frames: scene.rate, pitch: 1, gain: 1, bus: 1, when: 0 },
      { channels: 1, rate: 22050, frames: 11025, pitch: 0.91, gain: 0.65, when: 0.027 },
      { channels: 2, rate: scene.rate, frames: 6000, pitch: 1, gain: 1, when: 0.19 },
    ],
    dip: loading ? null : { from: 1, to: 0.05, when: 0.03, until: 0.28 },
  };
}
export function frontendBuffer(ctx, voice, index) {
  const b = ctx.createBuffer(voice.channels, voice.frames, voice.rate);
  for (let c = 0; c < voice.channels; c++) {
    const d = b.getChannelData(c);
    for (let i = 0; i < d.length; i++) d[i] = 0.16 * Math.sin(i * (index + 1) * 0.071 + c * 0.8) + 0.03 * Math.cos(i * 0.013);
  }
  return b;
}
export async function renderFrontendFixture(scene) {
  const s = frontendSpec(scene), ctx = new OfflineAudioContext(2, s.rate * s.seconds, s.rate);
  const player = new FrontendAudio(ctx); await player.ready;
  player.volumes(s.master, s.music, s.fx);
  for (const [i, v] of s.voices.entries()) {
    const handle = player.play(frontendBuffer(ctx, v, i), v);
    if (v.fadeEnd) handle.fade(v.fadeAt, v.fadeEnd);
  }
  if (s.dip) player.dip(s.dip.to, s.dip.until - s.dip.when, s.dip.when);
  await player.sync();
  const result = await ctx.startRendering();
  const encoded = []; let peak = 0;
  for (let c = 0; c < 2; c++) {
    const data = result.getChannelData(c), bytes = new Uint8Array(data.buffer);
    for (const v of data) peak = Math.max(peak, Math.abs(v));
    let text = ""; for (let i = 0; i < bytes.length; i++) text += String.fromCharCode(bytes[i]);
    encoded.push(btoa(text));
  }
  return { encoded, peak };
}
