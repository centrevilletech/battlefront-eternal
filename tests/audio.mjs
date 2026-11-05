// The browser offline gate and the engine gate share the same recorded
// inputs now that Rust owns the complete graph.
await import("./audio-engine-gate.mjs");
await import("./audio-codec-gate.mjs");
