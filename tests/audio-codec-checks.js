// Browser integration for the same frozen codec PCM used in CI. All asset
// loading, including optional HRIR WAV, goes through the required worker.
import { loadAudioPcm, openAudio } from "../src/audio/codec.js";
import { FrontendAudio } from "../src/audio/frontend.js";
import { Renderer } from "../src/audio/spatial.js";
import { compareAudio } from "./audio-golden.js";
export async function codecChecks(report = () => {}) {
  const manifest = await fetch("/tests/fixtures/audio-codecs/host.json").then((r) => r.json());
  const results = [];
  for (const scene of manifest.scenes) {
    try {
      const pcm = await loadAudioPcm(`/tests/fixtures/audio-codecs/${scene.file}`, { rate: scene.rate });
      const bytes = new Uint8Array(pcm.length * pcm.numberOfChannels * 4);
      for (let c = 0; c < pcm.numberOfChannels; c++) bytes.set(new Uint8Array(pcm.getChannelData(c).buffer), c * pcm.length * 4);
      const response = await fetch(`/tests/fixtures/audio-codecs/${scene.name}.f32.gz`);
      const stream = response.headers.get("Content-Encoding")?.includes("gzip") ? response.body : response.body.pipeThrough(new DecompressionStream("gzip"));
      const expected = new Uint8Array(await new Response(stream).arrayBuffer());
      const comparison = compareAudio(bytes, expected);
      let exact = bytes.length === expected.length;
      for (let i = 0; exact && i < bytes.length; i++) exact = bytes[i] === expected[i];
      const strict = scene.codec !== "opus" && scene.rate === scene.sourceRate;
      const pass = pcm.length === scene.frames && pcm.numberOfChannels === scene.channels && (strict ? exact : comparison.pass);
      const result = { name: scene.name, pass, detail: exact ? "bit-identical" : `${comparison.db.toFixed(2)} dBFS` };
      results.push(result); report(result);
    } catch (error) { const result = { name: scene.name, pass: false, detail: error.message }; results.push(result); report(result); }
  }
  // A real clock gives the producer time to refill its bounded window.
  // Offline render goldens cover the render phase separately.
  for (const kind of ["frontend", "bed", "cancel"]) {
    let ctx;
    try {
      ctx = new AudioContext({ sampleRate: kind === "frontend" ? 44100 : 48000 }); await ctx.resume();
      const file = kind === "frontend" ? "opus-hybrid.opus" : `opus-${kind === "bed" ? 8 : 2}ch.opus`;
      const asset = await openAudio(`/tests/fixtures/audio-codecs/${file}`, { rate: ctx.sampleRate });
      let ended = 0, failure;
      if (kind === "frontend") {
        const player = new FrontendAudio(ctx); await player.ready;
        player.fail = (error) => { failure = error; };
        // This track outlasts the ring. Background clip requests share
        // the worker while its 44.1 kHz producer must refill in real time.
        const preloads = Array.from({ length: 12 }, () => loadAudioPcm("/tests/fixtures/audio-codecs/flac24-stereo.flac", { rate: ctx.sampleRate }));
        const voice = await player.playStream(asset); voice.onended = () => ended++;
        await Promise.all(preloads);
        await new Promise((resolve) => setTimeout(resolve, (asset.duration + 0.25) * 1000));
        if (ended !== 1 || player.voices.size) throw Error("streamed front-end completion did not retire its PCM");
      } else {
        const renderer = new Renderer(ctx); renderer.onError = (error) => { failure = error; };
        await renderer.configure({ quality: "enhanced", output: "stereo", mono: false });
        const lane = renderer.stream(8, "music");
        const preparing = lane.prepare(asset);
        if (kind === "cancel") {
          lane.dispose();
          try { await preparing; throw Error("cancelled preparation resolved"); }
          catch (error) { if (error.name !== "AbortError") throw error; }
        } else {
          const pcm = await preparing; lane.start(pcm, ctx.currentTime, 1.07); renderer.flush();
          await new Promise((resolve) => setTimeout(resolve, 150));
          // Rebuild the live host while a producer is attached. The module
          // stays in the worklet; only the new output state is messaged.
          await renderer.configure({ quality: "original", output: "stereo", mono: true });
          await renderer.configure({ quality: "enhanced", output: "stereo", mono: false });
          await new Promise((resolve) => setTimeout(resolve, 550)); lane.dispose();
        }
      }
      asset.close();
      if (failure) throw failure;
      const result = { name: `stream-${kind}`, pass: true, detail: "worker and bounded Rust output" }; results.push(result); report(result);
    } catch (error) { const result = { name: `stream-${kind}`, pass: false, detail: error.message }; results.push(result); report(result); }
    finally { await ctx?.close(); }
  }
  // Exercise the optional WAV loader and its sixteen stereo filter
  // repacking too. The synthetic ears stand in for the optional dataset.
  let hrirContext;
  const fetchAsset = globalThis.fetch;
  try {
    const bytes = await fetchAsset("/tests/fixtures/audio-codecs/hrir-libsndfile16.wav").then((r) => r.arrayBuffer());
    globalThis.fetch = (url, options) => {
      if (!String(url).startsWith("/__codec_hrir__/")) return fetchAsset(url, options);
      return Promise.resolve(String(url).endsWith(".json")
        ? new Response(JSON.stringify({ rate: 48000 }), { headers: { "Content-Type": "application/json" } })
        : new Response(bytes.slice(0), { headers: { "Content-Type": "audio/wav" } }));
    };
    hrirContext = new AudioContext({ sampleRate: 44100 }); await hrirContext.resume();
    const renderer = new Renderer(hrirContext);
    const decoder = await renderer.loadDecoder("/__codec_hrir__");
    if (decoder.filters.length !== 16 || decoder.filters.some((f) => f.length !== 235 || f.sampleRate !== 44100)) throw Error("optional WAV decoder changed its filter format");
    await renderer.configure({ quality: "enhanced", output: "headphones", mono: true });
    if (!renderer.binaural) throw Error("optional WAV decoder did not reach the Rust output");
    const result = { name: "optional-hrir-loader", pass: true, detail: "sixteen WAV lanes, explicit rate/scale and Rust decoder" }; results.push(result); report(result);
  } catch (error) { const result = { name: "optional-hrir-loader", pass: false, detail: error.message }; results.push(result); report(result); }
  finally { globalThis.fetch = fetchAsset; await hrirContext?.close(); }
  return results;
}
