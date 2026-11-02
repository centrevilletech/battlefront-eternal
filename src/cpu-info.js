// Whether this PC's CPU is a low-end one, by what the browser says of it:
// four threads or fewer and 4 GB of memory or less (Chromium rounds memory
// to the nearest power of two, so up to 6 GB says 4). That is a laptop such
// as a 2014 A8-6410 (four 2 GHz cores sharing 15 W with the GPU) or a
// 2011 quad-core desktop, where the Enhanced sound stage alone keeps a
// third of a core busy at real-time priority, taking it from the page and
// the world worker. A first session there starts on Classic sound, whose
// light mix costs a fraction of that (Settings adoptHardwareDefaults,
// docs/audio.md). Without the memory figure (other browsers, Node) nothing
// is known, and nothing changes.
export function lowEndCpu(nav = globalThis.navigator) {
  const threads = Number(nav?.hardwareConcurrency) || 0;
  const memory = Number(nav?.deviceMemory) || 0;
  return threads > 0 && threads <= 4 && memory > 0 && memory <= 4;
}
