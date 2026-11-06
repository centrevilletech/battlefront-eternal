import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { gunzipSync } from "node:zlib";
import { installAudioHost } from "./audio-offline-host.js";
import { compareAudio, audioBytes, audioHash } from "./audio-golden.js";
import { renderAudioFixture, renderVoiceLife, renderStreamFixture } from "./audio-fixture.js";

import { renderFrontendFixture } from "./audio-frontend-fixture.js";

installAudioHost();
const fixtures = new URL("./fixtures/audio/", import.meta.url);
const manifest = JSON.parse(readFileSync(new URL("renders.json", fixtures)));
manifest.scenes.push(...JSON.parse(readFileSync(new URL("frontend.json", fixtures))).scenes);
for (const scene of manifest.scenes) test(`recorded audio: ${scene.name}`, async () => {
  const render = scene.render === "frontend" ? await renderFrontendFixture(scene) : scene.render === "stream" ? await renderStreamFixture(scene) : scene.life ? await renderVoiceLife(scene) : await renderAudioFixture(scene);
  const bytes = audioBytes(render.encoded);
  assert.ok(render.peak > 0.001, "render must contain sound");
  if (scene.strict) assert.equal(await audioHash(bytes), scene.sha256, "bit-identical recorded render");
  else {
    const expected = gunzipSync(readFileSync(new URL(scene.name + ".f32.gz", fixtures)));
    const result = compareAudio(bytes, expected);
    assert.ok(result.pass, `max difference ${result.db} dBFS; required below -96 dBFS`);
  }
});
