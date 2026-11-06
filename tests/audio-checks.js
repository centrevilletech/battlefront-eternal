import { renderAudioFixture, renderVoiceLife, renderStreamFixture } from "./audio-fixture.js";
import { renderFrontendFixture } from "./audio-frontend-fixture.js";
import { compareAudio, audioBytes, audioHash } from "./audio-golden.js";

// Run offline in Chromium, with the same fixtures as CI and no live native
// reference graph. Goldens were frozen before removing the browser DSP.
export async function audioChecks(options = {}) {
  const manifest = await fetch("/tests/fixtures/audio/renders.json").then((r) => r.json());
  const frontend = await fetch("/tests/fixtures/audio/frontend.json").then((r) => r.json());
  manifest.scenes.push(...frontend.scenes);
  const results = [];
  for (const scene of manifest.scenes) {
    if (options.name && scene.name !== options.name) continue;
    if (options.output && scene.output !== options.output) continue;
    if (options.quality && scene.quality !== options.quality) continue;
    try {
      const render = scene.render === "frontend" ? await renderFrontendFixture(scene) : scene.render === "stream" ? await renderStreamFixture(scene) : scene.life ? await renderVoiceLife(scene) : await renderAudioFixture(scene);
      const bytes = audioBytes(render.encoded);
      let result;
      if (scene.strict) result = { pass: await audioHash(bytes) === scene.sha256, peak: render.peak, detail: "bit-identical" };
      else {
        const compressed = await fetch(`/tests/fixtures/audio/${scene.name}.f32.gz`);
        if (!compressed.ok) throw new Error(`missing golden ${scene.name}`);
        const body = compressed.headers.get("Content-Encoding")?.includes("gzip") ? compressed.body : compressed.body.pipeThrough(new DecompressionStream("gzip"));
        const expected = new Uint8Array(await new Response(body).arrayBuffer());
        result = compareAudio(bytes, expected);
        result.detail = `max difference ${result.db.toFixed(2)} dBFS`;
      }
      results.push({ name: scene.name, ...result });
    } catch (error) { results.push({ name: scene.name, pass: false, detail: error.message }); }
    options.report?.(results.at(-1));
  }
  return results;
}
export { audioChecks as runAudioChecks };
