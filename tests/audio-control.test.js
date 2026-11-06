import test from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { gunzipSync } from "node:zlib";
import { installAudioHost } from "./audio-offline-host.js";
import { audioBytes, compareAudio } from "./audio-golden.js";
import { CONTROL_SCENES, renderControlFixture } from "./audio-control-fixture.js";

// The frozen samples and per-frame decisions came from the complete control
// phase. Checking both catches a changed voice choice that a quiet mix could
// otherwise conceal. The fixture contains synthetic samples and game facts.
installAudioHost();
const directory = new URL("./fixtures/audio-control/", import.meta.url);
const reference = JSON.parse(readFileSync(new URL("reference.json", directory)));
assert.deepEqual(reference.scenes.map((scene) => scene.name), CONTROL_SCENES.map((scene) => scene.name));

for (const scene of reference.scenes) test(`recorded sound controls: ${scene.name}`, async (context) => {
  const render = await renderControlFixture(scene);
  const actual = audioBytes(render.encoded);
  const expected = gunzipSync(readFileSync(new URL(`${scene.name}.f32.gz`, directory)));
  assert.equal(createHash("sha256").update(expected).digest("hex"), scene.sha256, "recorded PCM is intact");
  const result = compareAudio(actual, expected);
  context.diagnostic(`PCM: ${result.differences} differing samples, maximum difference ${result.error}, ${result.db} dBFS`);
  assert.ok(result.pass, `maximum difference ${result.db} dBFS; required below -96 dBFS`);
  const trace = JSON.stringify(render.trace);
  const recordedTrace = gunzipSync(readFileSync(new URL(`${scene.name}.trace.json.gz`, directory))).toString();
  assert.equal(createHash("sha256").update(recordedTrace).digest("hex"), scene.traceSha256, "recorded decision trace is intact");
  assert.equal(trace, recordedTrace, "voice ranking, stealing and rejections match every frame");
  assert.equal(render.seed, scene.seed, "gate and sample draws keep their recorded order");
  assert.ok(render.trace.every((row) => row[0] <= 4), "heard voice limit");
  assert.ok(render.trace.every((row) => row[0] + row[1] <= 8), "virtual voice pool limit");
  assert.ok(render.trace.some((row) => row[3] > 0), "script exercises stealing");
  assert.ok(render.trace.some((row) => row[4] > 0), "script exercises pool rejection");
  assert.ok(render.trace.some((row) => row[5] > 0), "script exercises distance culling");
});

// Cached speech completions can arrive before any returned transfer buffer
// is delivered to the page. The production controller must retain readiness
// ordering without allocating another frame arena or muting the device.
test("sound control transports a burst of cached stream completions", async () => {
  const render = await renderControlFixture({...CONTROL_SCENES[0],seconds:0.2,burstStreams:8});
  assert.ok(render.peak > 0.001,"the render phase received nonzero PCM");
  assert.equal(render.trace.length,12,"the complete control phase kept advancing");
});
