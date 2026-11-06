// Keep original art and glyph coverage. Generated lossless atlases share the
// source UVs; missing upgrades simply retain the imported native bitmap.
import { rendererFailure } from "./rust-render/failure.js";
import { pngImage, setPngSource } from "./texture-images.js";
export class RasterHudAssets {
  constructor(renderer) {
    this.renderer = renderer;
    this.queue = Promise.resolve();
    this.ready = fetch("/common/raster-hud/manifest.json")
      // Absence is a content fact: a 404, or the development server's HTML
      // page for an unknown path. A present but broken index stays a failure.
      .then((response) => {if(response.status===404||/text\/html/.test(response.headers.get("content-type")||""))return null;if(!response.ok)throw Error(`HUD atlas index: ${response.status}`);return response.json();});
    this.ready.catch(cause=>{this.renderer.resources.failure=rendererFailure(this.renderer,cause);});
  }

  upgrade(texture, url) {
    this.queue = this.queue.then(async () => {
      const manifest = await this.ready;
      const path = new URL(url, location.href).pathname;
      if (!path.startsWith("/common/")) return;
      const entry = manifest?.textures?.[path.slice("/common/".length)];
      if (!entry || texture.userData.rasterSource === entry.file) return;
      const width = entry.width * manifest.scale, height = entry.height * manifest.scale;
      // An oversized optional atlas keeps its already loaded native bitmap.
      // Check before decode so it cannot allocate unusable full-size pixels.
      if (Math.max(width, height) > this.renderer.capabilities.maxTextureSize) return;
      const atlasUrl = `/common/raster-hud/${entry.file}`;
      const image = await pngImage(atlasUrl, this.renderer.capabilities.maxTextureSize);
      try {
      if (image.width !== width || image.height !== height) throw Error(`HUD atlas dimensions: ${atlasUrl}`);
      await new Promise((resolve) => requestAnimationFrame(resolve));
      // Texture storage is immutable in WebGL2; retain the shared Texture
      // object and its sampling settings while reallocating its GPU storage.
      texture.dispose();
      // Layout still measures the native bitmap. The larger allocation uses
      // its same normalized UVs and must not enlarge glyphs or interface art.
      texture.userData.nativeWidth = entry.width;
      texture.userData.nativeHeight = entry.height;
      texture.isDataTexture = true;
      // No browser image conversion runs at upload time. Straight RGBA rows
      // preserve coverage, and source retention supports context restoration.
      texture.image = { data: image.data, width, height };
      setPngSource(texture, image, atlasUrl);
      texture.userData.rasterSource = entry.file;
      texture.needsUpdate = true;
      // Upload before the next interface frame can sample the resized atlas.
      this.renderer.initTexture(texture);
      } finally {image.release();}
    }).catch(cause=>{this.renderer.resources.failure=rendererFailure(this.renderer,cause);});
    return this.queue;
  }
}
