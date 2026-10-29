// The enhanced mode's upscaled sound, as tools/enhance_audio.py writes it
// (public/audio/enhanced/<sublevel>, gitignored): 48 kHz FLAC per sample,
// Opus per stream segment, music also upmixed to 5.1 and 7.1, and a
// manifest with each item's parity checks. Items that failed them are marked "fallback"
// and play their original. Without the folder, everything plays original.
//
// Samples load in the background once the enhanced mode is on; until one
// has, its original plays. An item stored quieter than it plays (its peaks
// passed full scale) is brought back up by its playback_db.

export class EnhancedAssets {
  constructor(ctx, base) {
    this.ctx = ctx;
    this.base = base;
    this.manifest = null;
    this.buffers = new Map();
    this.loading = null;
    this.stats = { samples: 0, loaded: 0, fallback: 0, failed: 0 };
  }

  static async open(ctx, root, sublevel) {
    const base = `${root}/enhanced/${sublevel}`;
    const manifest = await fetch(`${base}/manifest.json`)
      .then((r) => (r.ok ? r.json() : null))
      .catch(() => null);
    if (!manifest) return null;
    const assets = new EnhancedAssets(ctx, base);
    assets.manifest = manifest;
    const all = Object.values(manifest.samples || {});
    assets.stats.samples = all.length;
    assets.stats.fallback = all.filter((s) => s.fallback).length;
    return assets;
  }

  // Start decoding every sample (a few at a time).
  preload() {
    if (this.loading) return this.loading;
    const entries = Object.entries(this.manifest.samples || {}).filter(([, s]) => !s.fallback);
    let next = 0;
    const worker = async () => {
      while (next < entries.length) {
        const [id, entry] = entries[next++];
        try {
          const data = await fetch(`${this.base}/${entry.file}`).then((r) => r.arrayBuffer());
          const buffer = await this.ctx.decodeAudioData(data);
          scale(buffer, entry.playback_db);
          this.buffers.set(id, buffer);
          this.stats.loaded++;
        } catch {
          this.stats.failed++;
        }
      }
    };
    this.loading = Promise.all(Array.from({ length: 6 }, worker));
    return this.loading;
  }

  sample(id) {
    return this.buffers.get(id) || null;
  }

  // The file for a stream segment in this layout, and its playback gain.
  segment(stream, key, channels) {
    const entry = this.manifest.streams?.[stream]?.[key];
    if (!entry || entry.fallback) return null;
    const files = entry.files || {};
    let file = null;
    if (files.stereo) file = channels >= 8 ? files["7.1"] || files["5.1"] || files.stereo : channels >= 6 ? files["5.1"] || files.stereo : files.stereo;
    else file = Object.values(files)[0];
    return file ? { url: `${this.base}/${file}`, playback: entry.playback_db || 0 } : null;
  }
}

export function scale(buffer, db) {
  if (!db) return buffer;
  const g = 10 ** (db / 20);
  for (let c = 0; c < buffer.numberOfChannels; c++) {
    const d = buffer.getChannelData(c);
    for (let i = 0; i < d.length; i++) d[i] *= g;
  }
  return buffer;
}
