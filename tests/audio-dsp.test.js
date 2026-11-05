import test from "node:test";
// Primitive goldens complement the complete source-to-device render gate.
test("Rust DSP matches its recorded synthetic samples and readings", async () => { await import("./audio-dsp-parity.mjs"); });
