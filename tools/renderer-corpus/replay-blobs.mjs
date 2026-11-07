// Capture buffers are immutable. Repeated texture snapshots may share storage,
// but only after comparing every byte, including floating-point bit patterns.
// This function is also injected into the browser capture session.
export function replayBlobStore(target) {
  let owner, sequence, buckets;
  return buffer => {
    if (owner !== target.blobs) { owner = target.blobs; sequence = 0; buckets = new Map(); }
    const bytes = new Uint8Array(buffer.buffer, buffer.byteOffset, buffer.byteLength);
    let hash = 2166136261;
    for (let i = 0; i < bytes.length; i++) hash = Math.imul(hash ^ bytes[i], 16777619);
    const signature = `${buffer.constructor.name}:${bytes.length}:${hash}`;
    let keys = buckets.get(signature);
    for (const key of keys || []) {
      const saved = owner[key];
      if (!saved) continue;
      const previous = new Uint8Array(saved.buffer, saved.byteOffset, saved.byteLength);
      let equal = true;
      for (let i = 0; i < bytes.length; i++) if (bytes[i] !== previous[i]) { equal = false; break; }
      if (equal) return key;
    }
    const key = String(sequence++);
    owner[key] = buffer;
    if (!keys) buckets.set(signature, keys = []);
    keys.push(key);
    return key;
  };
}
