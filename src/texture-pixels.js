// CPU lamp and hue discovery reads the original PNG, including when the GPU
// samples compressed blocks. Rust preserves the old Canvas byte conversion;
// GPU uploads keep straight alpha and never use those converted CPU values.
import { pngSource, pngCpuPixels, pngImage } from "./texture-images.js";
import { rendererFailure } from "./rust-render/failure.js";
const entries = new WeakMap();

export function texturePixels(texture) {
  if (!texture) return null;
  const known = entries.get(texture);
  if (known?.failure) throw known.failure;
  if (known) return known.pixels;
  const source = pngSource(texture), png = source?.url || texture.userData?.png;
  if (png) {
    const entry = { pixels: null };
    // Preserve the asynchronous null-until-ready contract used by discovery.
    // Sampler clones can share decoded pixels without sharing their filters.
    entries.set(texture, entry);
    const ready = source ? pngCpuPixels(source) : pngImage(png, 16384, true);
    ready.then(image => {
      entry.pixels = { data: image.data, width: image.width, height: image.height };
    }).catch(cause => {entry.failure=rendererFailure(null,cause);});
    return null;
  }
  // Procedural texels are already CPU arrays supplied by presentation. They
  // have no PNG decode or Canvas conversion; retain their existing row order.
  const image = texture.image;
  if (!image?.data || !image.width || !image.height || texture.isCompressedTexture) return null;
  const pixels = { data: image.data, width: image.width, height: image.height };
  entries.set(texture, { pixels });
  return pixels;
}
