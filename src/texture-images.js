// Rust decodes PNG sources; scene objects retain texture descriptors.
import * as THREE from "three";
import { pngImage, decodePng } from "./texture-decode.js";
export { pngImage } from "./texture-decode.js";
import { rendererFailure } from "./rust-render/failure.js";

const sources = new WeakMap();
// Sampler clones share Three's Source object. Its encoded image is retained
// once, and can be collected when the last scene reference releases it.
export function pngSource(texture) { return sources.get(texture.source); }
export function setPngSource(texture, image, url) { sources.set(texture.source, { encoded: image.encoded, url }); }
export function copyPngSource(texture, from) { texture.source=from.source; }

// CPU reads share one converted image for sampler clones of the same source.
// Transfer a copy of the encoded input; its retained original is still needed
// for GPU restoration. The much larger decoded GPU array is never copied here.
export function pngCpuPixels(source) {
  return source.pixels ??= decodePng((source.encoded ? source.encoded.slice() : source.owner.source_encoded(source.id)).buffer, 16384, true)
    .then(image => ({ data: image.data, width: image.width, height: image.height }));
}

export class PngTextureLoader {
  constructor(manager, renderer) {
    this.manager = manager;
    this.renderer = renderer;
    this.failure = null;
  }

  load(url, onLoad, onProgress, onError) {
    // Preserve TextureLoader's descriptor, including its initially null image.
    // Rust consumes the decoded rows; no DataTexture upload marker is needed.
    const texture = new THREE.Texture();
    const path = this.manager.resolveURL(url);
    // Loading-manager completion covers decoding, too. Merely receiving the
    // file must not release a battle whose materials still hold empty images.
    this.manager.itemStart(path);
    pngImage(path, this.renderer.capabilities.maxTextureSize).then(async image => {
      try {
      texture.image = { data: image.data, width: image.width, height: image.height };
      setPngSource(texture, image, path);
      // Publish metadata before observers, which can upload synchronously.
      texture.needsUpdate = true;
      await onLoad?.(texture);
      // World loading awaits its paced GPU upload. Interface preloads keep
      // the worker rows until first use; no drawing-thread decode is needed.
      this.renderer.resources.retainSource(texture);
      } finally {image.release();}
    }).catch(cause => {
      // Preserve the first failure even when several resources fail together.
      this.failure ??= rendererFailure(this.renderer, cause);
      this.renderer.resources.failure=this.failure;
      this.manager.itemError(path);
      onError?.(this.failure);
    }).finally(() => this.manager.itemEnd(path));
    return texture;
  }
}
