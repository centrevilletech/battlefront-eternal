// Golden comparisons over synthetic fixtures. Strict cases retain hashes;
// native graphs with three or more summed inputs allow only -96 dBFS.
export function compareAudio(actual, expected, strict = false) {
  if (actual.byteLength !== expected.byteLength) throw new Error(`audio length ${actual.byteLength}, expected ${expected.byteLength}`);
  const a = new Float32Array(actual.buffer, actual.byteOffset, actual.byteLength / 4);
  const b = new Float32Array(expected.buffer, expected.byteOffset, expected.byteLength / 4);
  const bitsA = new Uint32Array(actual.buffer, actual.byteOffset, actual.byteLength / 4);
  const bitsB = new Uint32Array(expected.buffer, expected.byteOffset, expected.byteLength / 4);
  let peak = 0, error = 0, differences = 0;
  for (let i = 0; i < a.length; i++) {
    if (!Number.isFinite(a[i])) throw new Error(`non-finite audio at ${i}`);
    peak = Math.max(peak, Math.abs(a[i]));
    error = Math.max(error, Math.abs(a[i] - b[i]));
    if (bitsA[i] !== bitsB[i]) differences++;
  }
  return { pass: peak > 0.001 && (strict ? differences === 0 : error < 10 ** (-96 / 20)), peak, error, differences, db: error ? 20 * Math.log10(error) : -Infinity };
}
export function audioBytes(encoded) {
  const parts = encoded.map((text) => Uint8Array.from(atob(text), (c) => c.charCodeAt(0)));
  const result = new Uint8Array(parts.reduce((length, part) => length + part.length, 0));
  let at = 0; for (const part of parts) { result.set(part, at); at += part.length; }
  return result;
}
export async function audioHash(bytes) { return [...new Uint8Array(await crypto.subtle.digest("SHA-256", bytes))].map((b) => b.toString(16).padStart(2, "0")).join(""); }
