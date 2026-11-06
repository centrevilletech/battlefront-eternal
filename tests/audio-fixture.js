import { loadAudioDsp } from "../src/audio/wasm-dsp.js";

// Recorded synthetic inputs isolate render arithmetic from sound selection.
// The control phase has its own scripted game-fact corpus. These messages
// were captured before removing the old controller; no live twin is needed.
let recorded;
async function compressed(name) {
  const url = new URL(`./fixtures/audio/inputs/${name}.json.gz`, import.meta.url);
  let stream;
  if (typeof process !== "undefined" && process.versions?.node) {
    const bytes = await (await import("node:fs/promises")).readFile(url);
    stream = new Blob([bytes]).stream().pipeThrough(new DecompressionStream("gzip"));
  } else {
    const response = await fetch(url); if (!response.ok) throw Error(`missing recorded audio input ${name}`);
    // A development server can advertise gzip and let fetch decode it.
    // Static artifact hosts may instead deliver the compressed file itself.
    stream = response.headers.get("Content-Encoding")?.includes("gzip") ? response.body : response.body.pipeThrough(new DecompressionStream("gzip"));
  }
  return JSON.parse(await new Response(stream).text());
}
async function inputs() {
  recorded ||= Promise.all([compressed("scenes"), compressed("buffers")]).then(([manifest, blocks]) => {
    const arrays = new Map();
    const decode = (value) => {
      if (!value || typeof value !== "object") return value;
      if (value.typed) {
        const key = value.typed + value.sha;
        if (!arrays.has(key)) {
          const bytes = Uint8Array.from(atob(blocks[value.sha]), (c) => c.charCodeAt(0));
          arrays.set(key, new globalThis[value.typed](bytes.buffer));
        }
        return arrays.get(key);
      }
      if (Array.isArray(value)) return value.map(decode);
      return Object.fromEntries(Object.entries(value).map(([key, data]) => [key, decode(data)]));
    };
    return decode(manifest).scenes;
  });
  return recorded;
}
export async function renderAudioFixture(scene) {
  const record = (await inputs())[scene.name];
  if (!record) throw Error(`no frozen render input: ${scene.name}`);
  const ctx = new OfflineAudioContext(record.channels, Math.round(record.rate * record.seconds), record.rate);
  await ctx.audioWorklet.addModule(new URL("../src/audio/worklets/output.js?no-inline", import.meta.url));
  const dsp = await loadAudioDsp();
  // Clone only setup objects: captured PCM and command arrays stay immutable.
  // Posting without transfer lets repeated fixtures share those input arrays.
  const options = { ...record.node.options, processorOptions: { ...record.node.options.processorOptions, dsp } };
  let readyResolve, readyReject;
  const ready = new Promise((resolve, reject) => { readyResolve = resolve; readyReject = reject; });
  const node = new AudioWorkletNode(ctx, record.node.name, options);
  const waits = new Map(); let sequence = 1000;
  node.port.onmessage = ({ data }) => {
    if (data.type === "ready") readyResolve();
    else if (data.type === "error") { readyReject(Error(data.message)); for (const wait of waits.values()) wait.reject(Error(data.message)); }
    else if (data.type === "sync") { waits.get(data.id)?.resolve(); waits.delete(data.id); }
  };
  node.connect(ctx.destination); await ready;
  const sync = () => new Promise((resolve, reject) => { const id = ++sequence; waits.set(id, { resolve, reject }); node.port.postMessage({ type: "sync", id }); });
  const groups = new Map();
  for (const message of record.messages) { if (!groups.has(message.frame)) groups.set(message.frame, []); groups.get(message.frame).push(message.data); }
  for (const message of groups.get(0) || []) node.port.postMessage(message);
  for (const [frame, messages] of groups) if (frame > 0) ctx.suspend(frame / record.rate).then(async () => {
    for (const message of messages) node.port.postMessage(message);
    await sync(); await ctx.resume();
  });
  await sync(); const out = await ctx.startRendering();
  const encoded = []; let peak = 0;
  for (let c = 0; c < out.numberOfChannels; c++) {
    const values = out.getChannelData(c); for (const value of values) peak = Math.max(peak, Math.abs(value));
    const bytes = new Uint8Array(values.buffer, values.byteOffset, values.byteLength); let text = "";
    for (let i = 0; i < bytes.length; i += 8192) text += String.fromCharCode(...bytes.subarray(i, i + 8192));
    encoded.push(btoa(text));
  }
  return { encoded, peak, channels: out.numberOfChannels, rate: out.sampleRate };
}
export const renderVoiceLife = renderAudioFixture;
export const renderStreamFixture = renderAudioFixture;
