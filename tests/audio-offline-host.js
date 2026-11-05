// CI host for the same worklet and page handoff used by Chromium. This
// supplies scheduling and PCM storage only; the real module renders every
// sample. Browser offline checks exercise Web Audio's separate host too.
const processors = new Map();
let constructing;
class Port {
  constructor() { this.onmessage = null; this.pending = []; }
  postMessage(data) {
    // MessagePort snapshots reused event/readings buffers when posting.
    // Delivering their live objects loses earlier completions offline.
    data = structuredClone(data);
    const peer = this.peer;
    queueMicrotask(() => { if (peer.onmessage) peer.onmessage({ data }); else peer.pending.push(data); });
  }
  set onmessage(callback) { this.callback = callback; if (callback && this.pending) for (const data of this.pending.splice(0)) queueMicrotask(() => callback({ data })); }
  get onmessage() { return this.callback; }
}
class Buffer {
  constructor(channels, length, rate) { this.numberOfChannels = channels; this.length = length; this.sampleRate = rate; this.duration = length / rate; this.data = Array.from({ length: channels }, () => new Float32Array(length)); }
  getChannelData(channel) { return this.data[channel]; }
  copyToChannel(data, channel) { this.data[channel].set(data); }
}
class Node {
  constructor(ctx, name, options) {
    this.context = ctx; this.options = options;
    this.port = new Port(); const peer = new Port(); this.port.peer = peer; peer.peer = this.port;
    constructing = peer; globalThis.sampleRate = ctx.sampleRate;
    this.processor = new (processors.get(name))(options); constructing = null;
    this.output = Array.from({ length: options.outputChannelCount?.[0] || 0 }, () => new Float32Array(128));
    ctx.nodes.push(this);
  }
  connect() {} disconnect() {} addEventListener() {}
}
class Context {
  constructor(channels, length, rate) {
    this.sampleRate = rate; this.length = length; this.currentTime = 0; this.nodes = []; this.stops = []; this.channels = channels;
    this.destination = { maxChannelCount: channels };
    this.audioWorklet = { addModule: async (url) => { await import(url); } };
  }
  createBuffer(channels, length, rate) { return new Buffer(channels, length, rate); }
  suspend(time) { return new Promise((resolve) => this.stops.push({ frame: Math.round(time * this.sampleRate / 128) * 128, resolve })); }
  resume() { this.resumeRender?.(); return Promise.resolve(); }
  async startRendering() {
    const result = this.createBuffer(this.channels, this.length, this.sampleRate);
    for (let frame = 0; frame < this.length; frame += 128) {
      this.currentTime = frame / this.sampleRate;
      const stops = this.stops.filter((s) => s.frame === frame);
      if (stops.length) {
        const resumed = new Promise((resolve) => { this.resumeRender = resolve; });
        for (const stop of stops) stop.resolve();
        await resumed;
      }
      globalThis.currentFrame = frame;
      for (const node of this.nodes) node.processor.process([], [node.output]);
      const node = this.nodes[0];
      for (let c = 0; c < this.channels; c++) result.data[c].set(node.output[c].subarray(0, Math.min(128, this.length - frame)), frame);
    }
    return result;
  }
}
export function installAudioHost() {
  globalThis.AudioWorkletProcessor = class { constructor() { this.port = constructing; } };
  globalThis.registerProcessor = (name, processor) => processors.set(name, processor);
  globalThis.AudioWorkletNode = Node;
  globalThis.OfflineAudioContext = Context;
}
