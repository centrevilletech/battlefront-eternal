import test from "node:test";
import assert from "node:assert/strict";
import { installAudioHost } from "./audio-offline-host.js";
import { FrontendAudio } from "../src/audio/frontend.js";
import { decodeAudioAsset } from "../src/audio/decode.js";

installAudioHost();
test("front-end completion events retire PCM once and allow a slot to be reused", async () => {
  const ctx = new OfflineAudioContext(2, 48000 / 2, 48000), player = new FrontendAudio(ctx);
  await player.ready;
  const pcm = ctx.createBuffer(1, 800, 8000); pcm.getChannelData(0).fill(0.2);
  let ended = 0;
  const first = player.play(pcm, { when: 0, pitch: 1.5 }); first.onended = () => ended++;
  const loop = player.play(pcm, { when: 0.01, loop: true }); loop.onended = () => ended++;
  await player.sync();
  const at = ctx.suspend(0.2);
  const rendering = ctx.startRendering();
  await at;
  assert.equal(ended, 1);
  assert.equal(player.voices.size, 1);
  const replacement = player.play(pcm, { when: ctx.currentTime }); replacement.onended = () => ended++;
  assert.equal(player.voices.size, 2);
  loop.fade(ctx.currentTime, ctx.currentTime + 0.08);
  await player.sync();
  await ctx.resume();
  await rendering;
  await player.sync();
  assert.equal(ended, 3);
  assert.equal(player.voices.size, 0);
});

test("compressed asset I/O has one explicit context-rate contract", async () => {
  const bytes = new ArrayBuffer(8), result = { sampleRate: 44100 };
  let calls = 0;
  const context = { sampleRate: 44100, async decodeAudioData(input) { calls++; assert.equal(input, bytes); return result; } };
  assert.equal(await decodeAudioAsset(context, bytes), result);
  assert.equal(calls, 1);
  result.sampleRate = 48000;
  await assert.rejects(decodeAudioAsset(context, bytes), /decoder returned 48000 Hz, expected 44100 Hz/);
});

test("front-end module startup failure rejects readiness without an alternate graph", async () => {
  const ctx = new OfflineAudioContext(2, 128, 48000);
  ctx.audioWorklet.addModule = async () => { throw new Error("module unavailable"); };
  const player = new FrontendAudio(ctx);
  await assert.rejects(player.ready, /module unavailable/);
  assert.equal(player.isReady, false);
  assert.equal(ctx.nodes.length, 0);
});
